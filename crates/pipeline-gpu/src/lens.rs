//! Resident export lens stages (see pipeline-cpu lens_plan.rs and lens.wgsl).
use super::*;
use pipeline_cpu::{CaPlan, MapPlan, VignettePlan};

fn bits(values: &[f64]) -> impl Iterator<Item = u32> + '_ {
    values.iter().map(|&v| (v as f32).to_bits())
}

fn pipelines(ctx: &crate::GpuContext) -> &[wgpu::ComputePipeline; 3] {
    ctx.lens_pipelines.get_or_init(|| {
        let module = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("export lens"),
                source: wgpu::ShaderSource::Wgsl(include_str!("lens.wgsl").into()),
            });
        ["lateral_ca", "vignette", "remap"].map(|entry| {
            ctx.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: None,
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
        })
    })
}

impl Batch<'_> {
    fn lens_dispatch(
        &mut self,
        kernel: usize,
        src: &wgpu::Buffer,
        dst: &wgpu::Buffer,
        params: &[u32],
        count: u32,
    ) {
        let pipeline = pipelines(self.gpu.context())[kernel].clone();
        self.dispatch(&pipeline, src, dst, bytemuck::cast_slice(params), count);
    }

    pub(super) fn lateral_ca_impl(
        &mut self,
        tile: &ResidentTile,
        frame: Extent,
        plan: &CaPlan,
    ) -> EngineResult<ResidentTile> {
        let l = tile.layout;
        if l.channels != 3 {
            return Err(EngineError::invalid("lateral CA", "RGB tile required"));
        }
        let (ox, oy) = tile.coord.pixel_origin(TILE_SIZE);
        let layout = TileLayout { halo: 0, ..l };
        let mut p = vec![
            l.extent.width,
            l.extent.height,
            u32::from(l.halo),
            ox,
            oy,
            frame.width,
            frame.height,
        ];
        p.extend(bits(&plan.crop.map(f64::from)));
        p.extend(bits(&plan.center));
        p.extend(bits(&plan.coordinate_scale));
        p.extend(bits(&[plan.amount]));
        p.extend(bits(&plan.red));
        p.extend(bits(&plan.blue));
        let dst = self.buffer(layout.len() * 4)?;
        let src = self.storage(tile)?.clone();
        self.lens_dispatch(0, &src, &dst, &p, layout.len() as u32);
        Ok(self.tile(tile.coord, layout, dst))
    }

    pub(super) fn lens_gain_impl(
        &mut self,
        tile: &ResidentTile,
        frame: Extent,
        plan: &VignettePlan,
    ) -> EngineResult<ResidentTile> {
        let l = tile.layout;
        if l.channels != 3 || l.halo != 0 {
            return Err(EngineError::invalid(
                "vignetting",
                "halo-free RGB tile required",
            ));
        }
        let (ox, oy) = tile.coord.pixel_origin(TILE_SIZE);
        let gains = plan.profile.as_ref().map_or(&[][..], |p| &p.embedded[..]);
        if gains.len() > pipeline_cpu::MAX_EMBEDDED {
            return Err(EngineError::Unsupported {
                what: "embedded vignetting gains".into(),
            });
        }
        let mut p = vec![
            l.extent.width,
            l.extent.height,
            ox,
            oy,
            frame.width,
            frame.height,
            u32::from(plan.profile.is_some()) | u32::from(plan.manual.is_some()) << 1,
            gains.len() as u32,
        ];
        let profile = plan.profile.as_ref();
        p.extend(bits(&profile.map_or([0.; 3], |v| v.vignette)));
        p.extend(bits(&profile.map_or([0.; 2], |v| v.center)));
        p.extend(bits(&profile.map_or([1.; 2], |v| v.coordinate_scale)));
        p.extend(bits(&[profile.map_or(0., |v| v.amount)]));
        p.extend(bits(&plan.manual.unwrap_or([0., 1.])));
        p.extend(bits(&profile.map_or([0., 0., 1., 1.], |v| v.embedded_crop)));
        for g in gains {
            p.extend(bits(&g.center));
            p.extend(bits(&[g.radius]));
            p.extend(bits(&g.coefficients));
        }
        let dst = self.buffer(l.len() * 4)?;
        let src = self.storage(tile)?.clone();
        self.lens_dispatch(1, &src, &dst, &p, l.len() as u32);
        Ok(self.tile(tile.coord, l, dst))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn remap_impl(
        &mut self,
        frame: Extent,
        tiles: &HashMap<TileCoord, ResidentTile>,
        (first, end): (u32, u32),
        plan: &MapPlan,
        output: Extent,
        rows: std::ops::Range<u32>,
        coord: TileCoord,
    ) -> EngineResult<ResidentTile> {
        if first >= end || end > frame.height || rows.is_empty() || rows.end > output.height {
            return Err(EngineError::invalid("lens map", "invalid row ranges"));
        }
        let (w, h) = plan.output_extent(frame.width, frame.height);
        if (w, h) != (output.width, output.height) {
            return Err(EngineError::invalid("lens map", "output extent mismatch"));
        }
        let lens = plan.lens.as_ref();
        if lens.is_some_and(|l| l.embedded.len() > pipeline_cpu::MAX_EMBEDDED) {
            return Err(EngineError::Unsupported {
                what: "embedded lens warps".into(),
            });
        }
        let source = TileLayout {
            extent: Extent::new(frame.width, end - first),
            halo: 0,
            channels: 3,
        };
        let src = self.assemble(
            frame,
            TileCoord::new(coord.level, 0, 0),
            source,
            (0, i64::from(first)),
            1,
            tiles,
        )?;
        let layout = TileLayout {
            extent: Extent::new(w, rows.len() as u32),
            halo: 0,
            channels: 3,
        };
        let r = plan.crop;
        let (iw, ih) = (frame.width as f32, frame.height as f32);
        let (cw, ch) = ((r.right - r.left) * iw, (r.bottom - r.top) * ih);
        let (cx, cy) = ((r.left + r.right) * iw / 2., (r.top + r.bottom) * ih / 2.);
        let (sin, cos) = plan.angle.to_radians().sin_cos();
        let sample = lens.and_then(|l| l.sample.as_ref());
        let flags = u32::from(plan.transform.is_some())
            | u32::from(lens.is_some()) << 1
            | u32::from(sample.is_some()) << 2;
        let mut p = vec![
            frame.width,
            frame.height,
            first,
            end - first,
            w,
            h,
            rows.start,
            rows.len() as u32,
            flags,
            lens.map_or(0, |l| l.embedded.len() as u32),
        ];
        p.extend([cw, ch, cx, cy, sin, cos].map(f32::to_bits));
        let t = plan.transform;
        p.extend(bits(&t.map_or([0.; 2], |t| t.offset)));
        p.extend(bits(&t.map_or([0., 1.], |t| t.rotate)));
        p.extend(bits(&t.map_or([1.; 2], |t| t.scale)));
        p.extend(bits(&t.map_or([0.; 2], |t| t.perspective)));
        p.extend(bits(&[lens.map_or(0., |l| l.manual_k1)]));
        p.extend(bits(&sample.map_or([0.; 3], |s| s.k)));
        p.extend(bits(&sample.map_or([0.; 2], |s| s.p)));
        p.extend(bits(&sample.map_or([0.; 2], |s| s.center)));
        p.extend(bits(&[sample.map_or(1., |s| s.distortion_scale)]));
        p.extend(bits(&sample.map_or([0.; 2], |s| s.radial_odd)));
        p.extend(bits(&sample.map_or([1.; 2], |s| s.coordinate_scale)));
        p.extend(bits(&[sample.map_or(0., |s| s.amount)]));
        p.extend(bits(&[lens.map_or(0., |l| l.distortion)]));
        p.extend(bits(&lens.map_or([0., 0., 1., 1.], |l| l.embedded_crop)));
        for warp in lens.map_or(&[][..], |l| &l.embedded[..]) {
            p.extend(bits(&warp.k));
            p.extend(bits(&warp.center));
            p.extend(bits(&[warp.radius]));
        }
        let dst = self.buffer(layout.len() * 4)?;
        let src_buffer = self.storage(&src)?.clone();
        self.lens_dispatch(2, &src_buffer, &dst, &p, layout.plane_len() as u32);
        drop(src);
        Ok(self.tile(coord, layout, dst))
    }
}
