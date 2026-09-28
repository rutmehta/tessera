use engine_api::{id::ImageId,jobs::CancellationToken,recipe::{DevelopSettings,settings::WhiteBalanceMode}};
use image_core::{RawImage,Renderer,RendererConfig,TileCache,PixelRect,RenderOutput};
use pipeline_cpu::CameraLinearProxy;
use std::{sync::Arc,time::Instant};
fn main() {
    let path="/Volumes/betterSSD/tessera-validation/m258-current/visible-run-f1d13c11/fixture/sony-arw.ARW";
    let original_bytes=std::fs::read(path).unwrap();
    let original=RawImage::open(ImageId(910),path).unwrap();
    let encoded=std::fs::read("/Volumes/betterSSD/tessera-validation/smart-previews/codec/sony-camera-linear.clp").unwrap();
    let decoded=CameraLinearProxy::decode_persistent(&encoded).unwrap();
    let proxy=RawImage::from_camera_linear_proxy(ImageId(910),ImageId(911),Arc::new(decoded.proxy)).unwrap();
    let base=DevelopSettings::default();
    let extent=Renderer::output_extent(&original,&base,1).unwrap();
    assert_eq!(extent,Renderer::output_extent(&proxy,&base,0).unwrap());
    let rect=PixelRect::full(extent);
    let gpu=Arc::new(pipeline_gpu::GpuContext::new().unwrap());
    let mut results=Vec::new();
    for round in 0..2 {
        for route in ["original_cpu","proxy_cpu","original_metal_resident_readback"] {
            let config=RendererConfig::default();
            let start=Instant::now();
            let renderer=if route=="original_metal_resident_readback" {
                Renderer::with_ops(Arc::new(pipeline_gpu::GpuStageOp::with_cache_budget(gpu.clone(),config.cache_budget_bytes)),Arc::new(TileCache::new(config.cache_budget_bytes)),config)
            } else { Renderer::new(config) };
            let setup_ms=start.elapsed().as_secs_f64()*1000.;
            let (image,level)=if route=="proxy_cpu" {(&proxy,0)} else {(&original,1)};
            for state in ["cold_default","warm_default","warm_wb_exposure_edit"] {
                let mut settings=base.clone();
                if state=="warm_wb_exposure_edit" {
                    settings.white_balance.mode=WhiteBalanceMode::Custom;
                    settings.white_balance.temperature=4200.;
                    settings.white_balance.tint=13.;
                    settings.tone.exposure=0.7;
                }
                let start=Instant::now();
                let tiles=if route=="original_metal_resident_readback" {
                    renderer.render_resident_region(image,&settings,level,rect,&CancellationToken::new()).unwrap().expect("default original lens recipe must admit resident rendering")
                } else { renderer.render_region_as(image,&settings,level,rect,RenderOutput::Display).unwrap() };
                let ms=start.elapsed().as_secs_f64()*1000.;
                let mut sample_count=0usize;
                let mut checksum=0u64;
                for tile in &tiles {
                    let samples=tile.samples::<u8>().unwrap();
                    sample_count+=samples.len();
                    checksum+=samples.iter().map(|v|u64::from(*v)).sum::<u64>();
                }
                assert!(sample_count>0);
                let row=serde_json::json!({"round":round,"route":route,"state":state,"ms":ms,"renderer_setup_ms":setup_ms,"dimensions":[extent.width,extent.height],"level":level,"tiles":tiles.len(),"sample_count":sample_count,"checksum":checksum});
                println!("{row}");results.push(row);
            }
        }
    }
    assert_eq!(original_bytes,std::fs::read(path).unwrap());
    std::fs::write("/Volumes/betterSSD/tessera-validation/smart-previews/image-core/benchmark-results.json",serde_json::to_vec_pretty(&serde_json::json!({"gpu":gpu.adapter_info.name,"output":"encoded display tiles; Metal includes readback, not app zero-copy presentation", "rounds":2,"states_per_renderer":3,"results":results})).unwrap()).unwrap();
}
