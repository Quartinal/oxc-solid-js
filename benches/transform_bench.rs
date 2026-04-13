use std::time::Instant;

use oxc_solid_js_compiler::{transform, TransformOptions};

fn collect_sources() -> Vec<(String, String)> {
    let mut sources = Vec::new();

    // Collect DOM fixture code.js files
    let fixture_dir = "submodules/dom-expressions/packages/babel-plugin-jsx-dom-expressions/test/__dom_fixtures__";
    if let Ok(entries) = std::fs::read_dir(fixture_dir) {
        for entry in entries.flatten() {
            let code_path = entry.path().join("code.js");
            if code_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&code_path) {
                    sources.push((code_path.display().to_string(), content));
                }
            }
        }
    }

    // Collect solid-primitives .tsx files
    for entry in glob_walk("benchmark/solid-primitives/packages") {
        if let Ok(content) = std::fs::read_to_string(&entry) {
            sources.push((entry, content));
        }
    }

    sources
}

fn glob_walk(dir: &str) -> Vec<String> {
    let mut result = Vec::new();
    walk_dir(dir, &mut result);
    result
}

fn walk_dir(dir: &str, result: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path.display().to_string(), result);
        } else if let Some(ext) = path.extension() {
            if ext == "tsx" || ext == "jsx" {
                result.push(path.display().to_string());
            }
        }
    }
}

fn main() {
    let sources = collect_sources();
    let total_bytes: usize = sources.iter().map(|(_, s)| s.len()).sum();
    println!(
        "Collected {} source files ({:.1} KB total)",
        sources.len(),
        total_bytes as f64 / 1024.0
    );

    // Warmup: 3 iterations
    for _ in 0..3 {
        for (_, source) in &sources {
            let _ = transform(source, Some(TransformOptions::solid_defaults()));
        }
    }

    // Benchmark: 100 iterations
    let iterations = 100;
    let start = Instant::now();
    for _ in 0..iterations {
        for (_, source) in &sources {
            let _ = std::hint::black_box(transform(
                std::hint::black_box(source),
                Some(TransformOptions::solid_defaults()),
            ));
        }
    }
    let elapsed = start.elapsed();

    let total_transforms = iterations * sources.len();
    let per_transform = elapsed / total_transforms as u32;
    let throughput_mb_s = (total_bytes * iterations) as f64 / elapsed.as_secs_f64() / 1_048_576.0;

    println!("--- Results ---");
    println!(
        "Total time:      {:?} ({} iterations × {} files)",
        elapsed,
        iterations,
        sources.len()
    );
    println!("Per transform:   {:?}", per_transform);
    println!("Throughput:      {:.2} MB/s", throughput_mb_s);
    println!("Total transforms: {}", total_transforms);
}
