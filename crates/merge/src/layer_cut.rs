//! Deterministic sequential binary s/t cuts on a bounded-resolution grid.
use crate::LinearImage;
use std::collections::VecDeque;
#[derive(Clone)]
struct Edge {
    to: usize,
    rev: usize,
    cap: f64,
}
struct Graph {
    edges: Vec<Vec<Edge>>,
    level: Vec<i32>,
    next: Vec<usize>,
}
impl Graph {
    fn new(n: usize) -> Self {
        Self {
            edges: vec![vec![]; n],
            level: vec![0; n],
            next: vec![0; n],
        }
    }
    fn add(&mut self, a: usize, b: usize, cap: f64) {
        let ar = self.edges[a].len();
        let br = self.edges[b].len();
        self.edges[a].push(Edge {
            to: b,
            rev: br,
            cap,
        });
        self.edges[b].push(Edge {
            to: a,
            rev: ar,
            cap: 0.,
        });
    }
    fn push(&mut self, v: usize, t: usize, f: f64) -> f64 {
        if v == t {
            return f;
        }
        while self.next[v] < self.edges[v].len() {
            let i = self.next[v];
            let e = self.edges[v][i].clone();
            if e.cap > 1e-10 && self.level[e.to] == self.level[v] + 1 {
                let d = self.push(e.to, t, f.min(e.cap));
                if d > 1e-10 {
                    self.edges[v][i].cap -= d;
                    self.edges[e.to][e.rev].cap += d;
                    return d;
                }
            }
            self.next[v] += 1;
        }
        0.
    }
    fn cut(&mut self, s: usize, t: usize) -> Vec<bool> {
        loop {
            self.level.fill(-1);
            self.level[s] = 0;
            let mut q = VecDeque::from([s]);
            while let Some(v) = q.pop_front() {
                for e in &self.edges[v] {
                    if e.cap > 1e-10 && self.level[e.to] < 0 {
                        self.level[e.to] = self.level[v] + 1;
                        q.push_back(e.to);
                    }
                }
            }
            if self.level[t] < 0 {
                break;
            }
            self.next.fill(0);
            while self.push(s, t, 1e30) > 1e-10 {}
        }
        self.level.iter().map(|v| *v >= 0).collect()
    }
}
pub(super) fn masks(images: &[LinearImage], coverage: &[Vec<bool>]) -> Vec<Vec<f32>> {
    let (w, h) = (images[0].width, images[0].height);
    let step = w.max(h).div_ceil(160);
    let (gw, gh) = (w.div_ceil(step), h.div_ceil(step));
    let n = gw * gh;
    let sample = |i: usize| {
        ((i / gw * step + step / 2).min(h - 1)) * w + (i % gw * step + step / 2).min(w - 1)
    };
    let mut owner = vec![usize::MAX; n];
    for k in 0..images.len() {
        let mut graph = Graph::new(n + 2);
        let (s, t) = (n, n + 1);
        for i in 0..n {
            let p = sample(i);
            let old = owner[i] != usize::MAX;
            let new = coverage[k][p];
            if old && !new {
                graph.add(s, i, 1e12)
            } else if new && !old {
                graph.add(i, t, 1e12)
            } else if old && new {
                graph.add(s, i, 0.00001);
            }
            for j in [
                (i % gw + 1 < gw).then_some(i + 1),
                (i / gw + 1 < gh).then_some(i + gw),
            ]
            .into_iter()
            .flatten()
            {
                let q = sample(j);
                let mut cost = 0.001;
                for v in [i, j] {
                    let at = sample(v);
                    if owner[v] != usize::MAX && coverage[k][at] {
                        for c in 0..3 {
                            cost += (images[owner[v]].pixels[at][c] as f64
                                - images[k].pixels[at][c] as f64)
                                .abs();
                        }
                    }
                }
                if (old || new) && (owner[j] != usize::MAX || coverage[k][q]) {
                    graph.add(i, j, cost);
                    graph.add(j, i, cost);
                }
            }
        }
        let cut = graph.cut(s, t);
        for i in 0..n {
            if !cut[i] && coverage[k][sample(i)] {
                owner[i] = k;
            }
        }
    }
    let mut masks = vec![vec![0.; w * h]; images.len()];
    for i in 0..w * h {
        let k = owner[(i / w / step) * gw + (i % w / step)];
        let chosen = if k != usize::MAX && coverage[k][i] {
            Some(k)
        } else {
            coverage.iter().position(|c| c[i])
        };
        if let Some(k) = chosen {
            masks[k][i] = 1.;
        }
    }
    masks
}
