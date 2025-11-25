#![allow(dead_code)]

use std::{
    fs::File,
    io::{
        BufReader,
        BufWriter,
        Write,
    },
    path::Path,
};

use eyre::{
    Context,
    Error,
};
use heck::ToShoutySnakeCase;
use palette::LinSrgb;
use serde::Deserialize;

pub fn create_constants_module(
    materials_json: impl AsRef<Path>,
    materials_rs: impl AsRef<Path>,
) -> Result<(), Error> {
    let materials_json = materials_json.as_ref();
    let materials_rs = materials_rs.as_ref();

    println!("cargo:rerun-if-changed={}", materials_json.display());

    let reader = BufReader::new(
        File::open(materials_json)
            .wrap_err_with(|| format!("Could not open file: {}", materials_json.display()))?,
    );
    let entries: Vec<Entry> = serde_json::from_reader(reader)
        .wrap_err_with(|| format!("Could parse JSON: {}", materials_json.display()))?;

    let mut writer = BufWriter::new(
        File::create(materials_rs)
            .wrap_err_with(|| format!("Could not write file: {}", materials_rs.display()))?,
    );

    for entry in &entries {
        let Entry {
            name,
            color,
            metalness,
            roughness,
            references,
            ..
        } = entry;

        let Some(color) = color
            .iter()
            .find(|color| color.color_space == ColorSpace::SrgbLinear)
        else {
            tracing::warn!("materials.json entry '{}' has no linear color", entry.name);
            continue;
        };
        let albedo = LinSrgb::from(color.color);

        let constant_name = entry.name.to_shouty_snake_case();

        writeln!(
            &mut writer,
            r#"
/// # {name}
///
/// - Color (linear SRGB): `[{}, {}, {}]`
/// - Metalness: {metalness}
/// - Roughness: {roughness}
///"#,
            albedo.red, albedo.green, albedo.blue,
        )?;

        if !references.is_empty() {
            writeln!(&mut writer, "/// ## References")?;
            writeln!(&mut writer, "///")?;
            for reference in references {
                writeln!(
                    &mut writer,
                    "/// - [{}]({})",
                    reference.title, reference.url
                )?;
            }
        }

        writeln!(
            &mut writer,
            r#"
pub const {constant_name}: MaterialPreset = MaterialPreset {{
    name: {name:?},
    albedo: LinSrgb::new({:?}, {:?}, {:?}),
    metallic: {metalness:?},
    roughness: {roughness:?},
}};
"#,
            albedo.red, albedo.green, albedo.blue,
        )?;
    }

    writeln!(&mut writer, "pub const ALL: &[&MaterialPreset] = &[")?;
    for entry in &entries {
        let constant_name = entry.name.to_shouty_snake_case();
        writeln!(&mut writer, "    &{constant_name},")?;
    }
    writeln!(&mut writer, "];")?;

    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    name: String,
    #[serde(default)]
    color: Vec<Color>,
    metalness: f32,
    #[serde(default)]
    specular_color: Vec<SpecularColor>,
    roughness: f32,
    #[serde(default)]
    complex_ior: Vec<ComplexIor>,
    #[serde(default)]
    density: Vec<f32>,
    #[serde(default)]
    category: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    references: Vec<Reference>,
    #[serde(default)]
    images: Vec<Image>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Color {
    color_space: ColorSpace,
    color: [f32; 3],
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ColorSpace {
    SrgbLinear,
    Acescg,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpecularColor {
    format: Vec<SpecularColorFormat>,
    color: Vec<Color>,
}

#[derive(Debug, Deserialize)]
enum SpecularColorFormat {
    #[serde(rename = "Gulbrandsen")]
    Gulbrandsen,
    #[serde(rename = "F82")]
    F82,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComplexIor {
    color_space: ColorSpace,
    n: [f32; 3],
    k: [f32; 3],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reference {
    title: String,
    author: Option<String>,
    url: String,
    accessed: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Image {
    format: ImageFormat,
    #[serde(rename = "300")]
    _300: String,
    #[serde(rename = "600")]
    _600: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ImageFormat {
    Jpeg,
    Avif,
}
