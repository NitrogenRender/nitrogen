extern crate shaderc;

use shaderc::*;

use std::env;
use std::path::PathBuf;

use std::collections::HashSet;
use std::fs;

fn main() {
    let project_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let examples = fs::read_dir(project_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir());

    for example in examples {
        let example_dir = example.path();

        let shaders_dir = example_dir.join("shaders");

        let mut vertex_shaders = HashSet::new();
        let mut fragment_shaders = HashSet::new();
        let mut geometry_shaders = HashSet::new();
        let mut compute_shaders = HashSet::new();

        for entry in fs::read_dir(shaders_dir).unwrap() {
            if entry.is_err() {
                continue;
            }

            let entry = entry.unwrap();
            let path = entry.path();

            let extension = if let Some(ext) = path.extension() {
                ext
            } else {
                continue;
            };

            if extension == "frag" || extension == "hlsl" {
                fragment_shaders.insert(path.clone());
            }
            if extension == "vert" || extension == "hlsl" {
                vertex_shaders.insert(path.clone());
            }
            if extension == "geom" || extension == "hlsl" {
                geometry_shaders.insert(path.clone());
            }
            if extension == "comp" || extension == "hlsl" {
                compute_shaders.insert(path.clone());
            }
        }

        let out_base = PathBuf::from(env::var("OUT_DIR").unwrap());
        let out_base = out_base.join(example.path().file_name().unwrap());

        fs::create_dir_all(&out_base);

        let mut compiler = Compiler::new().unwrap();

        for shader in vertex_shaders {
            compile(&mut compiler, shader, &out_base, ShaderKind::Vertex);
        }
        for shader in fragment_shaders {
            compile(&mut compiler, shader, &out_base, ShaderKind::Fragment);
        }
        for shader in geometry_shaders {
            compile(&mut compiler, shader, &out_base, ShaderKind::Geometry);
        }
        for shader in compute_shaders {
            compile(&mut compiler, shader, &out_base, ShaderKind::Compute);
        }
    }

}

pub fn compile(compiler: &mut Compiler, path: PathBuf, out_base: &PathBuf, kind: ShaderKind) {
    let contents = match fs::read_to_string(path.clone()) {
        Ok(c) => c,
        Err(_) => return,
    };

    let lang = if path.clone().extension().unwrap() == "hlsl" {
        SourceLanguage::HLSL
    } else {
        SourceLanguage::GLSL
    };

    let entry = match (lang, kind) {
        (SourceLanguage::HLSL, ShaderKind::Vertex) => "VertexMain",
        (SourceLanguage::HLSL, ShaderKind::Fragment) => "FragmentMain",
        (SourceLanguage::HLSL, ShaderKind::Geometry) => "GeometryMain",
        (SourceLanguage::HLSL, ShaderKind::Compute) => "ComputeMain",
        (SourceLanguage::GLSL, _) => "main",
        _ => return,
    };

    eprintln!("entry point: {}", entry);

    if lang == SourceLanguage::HLSL {
        if !contents.contains(entry) {
            return;
        }
    }

    let artifact = compile_to_spirv(
        compiler,
        path.to_str().unwrap(),
        &contents,
        kind,
        entry,
        lang,
    );

    let out_name = {
        let mut new_name = path.file_name().unwrap().to_string_lossy().to_string();

        if lang == SourceLanguage::HLSL {
            new_name = new_name + match kind {
                ShaderKind::Vertex => ".vert",
                ShaderKind::Fragment => ".frag",
                ShaderKind::Geometry => ".geom",
                ShaderKind::Compute => ".comp",
                _ => unreachable!(),
            };
        }

        let new_name = new_name + ".spirv";
        let base = PathBuf::from(out_base);
        base.join(new_name)
    };

    if let Some(data) = artifact {
        fs::write(out_name, data);
    };
}

pub fn compile_to_spirv(
    compiler: &mut Compiler,
    file_name: &str,
    source: &str,
    kind: ShaderKind,
    entry: &str,
    lang: SourceLanguage,
) -> Option<Vec<u8>> {
    let mut options = CompileOptions::new().unwrap();

    options.set_source_language(lang);

    //options.set_optimization_level(OptimizationLevel::Performance);

    options.set_warnings_as_errors();

    let artifact = compiler.compile_into_spirv(source, kind, file_name, entry, Some(&options));

    match artifact {
        Ok(data) => {
            // SPIR-V assembly output. In case something gets weird...
            /*
            let text = compiler.compile_into_spirv_assembly(
                source,
                kind,
                file_name,
                entry,
                Some(&options),
            ).unwrap().as_text();

            eprintln!("{}", text);
            */

            Some(data.as_binary_u8().to_owned())
        }
        Err(e) => {
            eprintln!("{:?}", e);
            None
        }
    }
}
