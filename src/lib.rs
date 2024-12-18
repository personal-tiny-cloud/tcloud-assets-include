// This file is part of the Tiny Cloud project.
// You can find the source code of every repository here:
//		https://github.com/personal-tiny-cloud
//
// Copyright (C) 2024  hex0x0000
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Email: hex0x0000@protonmail.com

use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use minify_html::minify;
use minify_html::Cfg;
use oxc_allocator::Allocator;
use oxc_codegen::{CodeGenerator, CodegenOptions};
use oxc_minifier::{CompressOptions, Minifier, MinifierOptions};
use oxc_parser::Parser;
use oxc_span::SourceType;

static NO_MANGLE: OnceLock<Vec<&str>> = OnceLock::new();

fn set_nomangle(files: Vec<&'static str>) {
    if !files.is_empty() && NO_MANGLE.get().is_none() {
        NO_MANGLE
            .set(files)
            .expect("Failed to set NO_MANGLE file list: already initialiazed")
    }
}

fn check_nomangle(file: &str) -> bool {
    if let Some(nomangle) = NO_MANGLE.get() {
        nomangle.contains(&file)
    } else {
        false
    }
}

static OTHER_EXTENSIONS: OnceLock<Vec<&str>> = OnceLock::new();

fn set_other_extensions(ext: Vec<&'static str>) {
    if !ext.is_empty() && OTHER_EXTENSIONS.get().is_none() {
        OTHER_EXTENSIONS
            .set(ext)
            .expect("Failed to set OTHER_EXTENSIONS list: already initialiazed")
    }
}

fn check_extension(file: &str) -> bool {
    if let Some(other_extensions) = OTHER_EXTENSIONS.get() {
        for extension in other_extensions {
            if file.ends_with(extension) {
                return true;
            }
        }
    }
    false
}

fn get_filename(path: &Path) -> &str {
    path.iter().last().unwrap().to_str().unwrap()
}

fn minify_js(path: &PathBuf, src: &str) -> String {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, src, SourceType::cjs()).parse();
    let mut program = ret.program;
    let options = MinifierOptions {
        mangle: !check_nomangle(get_filename(path)),
        compress: CompressOptions::default(),
    };
    let ret = Minifier::new(options).build(&allocator, &mut program);
    CodeGenerator::new()
        .with_options(CodegenOptions {
            minify: true,
            ..CodegenOptions::default()
        })
        .with_mangler(ret.mangler)
        .build(&program)
        .code
}

fn minify_css(path: &str, src: &str) -> String {
    let mut stylesheet = StyleSheet::parse(src, ParserOptions::default())
        .unwrap_or_else(|e| panic!("Invalid CSS file '{path}', cannot parse it: {e}"));
    stylesheet
        .minify(MinifyOptions::default())
        .unwrap_or_else(|e| panic!("Cannot minify CSS file '{path}': {e}"));
    stylesheet
        .to_css(PrinterOptions {
            minify: true,
            ..PrinterOptions::default()
        })
        .unwrap_or_else(|e| panic!("Cannot get minified CSS of file '{path}': {e}"))
        .code
}

fn minify_html(path: &str, src: &str) -> String {
    String::from_utf8(minify(src.as_bytes(), &Cfg::default()))
        .unwrap_or_else(|e| panic!("Failed to minify HTML file '{path}': {e}"))
}

fn handle_file(file: PathBuf, out_dir: &Path) {
    let path = file.to_str().expect("Invalid path UTF-8");

    // If it has an accepted extension it is copied without modification
    if check_extension(get_filename(&file)) {
        let mut new_file_path: PathBuf = out_dir.into();
        new_file_path.push(file.file_name().unwrap());
        fs::copy(&file, &new_file_path).unwrap_or_else(|_| {
            panic!(
                "Failed to copy file from {path} to {}",
                new_file_path.display()
            )
        });
        return;
    }

    // If it is a Web File it is minified and then written
    // If it's none of them the file is ignored
    let minified = if path.ends_with(".css") {
        minify_css(
            path,
            &fs::read_to_string(&file).unwrap_or_else(|e| panic!("Failed to read {path}: {e}")),
        )
    } else if path.ends_with(".js") {
        minify_js(
            &file,
            &fs::read_to_string(&file).unwrap_or_else(|e| panic!("Failed to read {path}: {e}")),
        )
    } else if path.ends_with(".html") {
        minify_html(
            path,
            &fs::read_to_string(&file).unwrap_or_else(|e| panic!("Failed to read {path}: {e}")),
        )
    } else {
        return;
    };
    let mut new_file_path: PathBuf = out_dir.into();
    new_file_path.push(file.file_name().unwrap());
    fs::write(&new_file_path, minified).unwrap_or_else(|_| {
        panic!(
            "Failed to write minified file {}",
            new_file_path.to_str().unwrap()
        )
    });
}

fn handle_directory(directory: PathBuf, out_dir: &PathBuf) {
    let mut new_dir = out_dir.clone();
    new_dir.push(directory.file_name().unwrap());
    fs::create_dir_all(&new_dir)
        .unwrap_or_else(|_| panic!("Failed to create {}", directory.display()));
    for direntry in fs::read_dir(&directory)
        .unwrap_or_else(|_| panic!("Failed to read files of {}", directory.display()))
        .flatten()
    {
        if let Ok(file_type) = direntry.file_type() {
            if file_type.is_dir() {
                handle_directory(direntry.path(), &new_dir);
            } else if file_type.is_file() {
                handle_file(direntry.path(), &new_dir);
            }
        }
    }
}

/// Copies assets (web files and/or binaries) into OUT_DIR.
/// They can be then included into the executable with [`include_str`] or [`include_bytes`].
///
/// By default, files are ignored unless they end with `.html`, `.js`, or `.css`. If you want to
/// add some other binary files you can specify their extension or ending in `other_extensions`.
///
/// HTML, JS, and CSS files will be minified to avoid using too much space. JavaScript
/// files are also mangled, which means that variables are shrinked to occupy less space.
/// If this behavior breaks some of your scripts, you can disable it for a specific script by
/// specifying its filename in the `no_mangle` argument.
///
/// - `path`: Path to the assets (relative to the root of the project).
/// - `other_extensions`: Files to include other than .html, .js or .css files (will be just copied).
/// - `no_mangle`: Specify which JS files should not be mangled.
pub fn include(path: &str, other_extensions: Vec<&'static str>, no_mangle: Vec<&'static str>) {
    set_other_extensions(other_extensions);
    set_nomangle(no_mangle);
    let out_dir =
        PathBuf::from(env::var_os("OUT_DIR").expect("Failed to get OUT_DIR env variable"));
    handle_directory(PathBuf::from(path), &out_dir);
    println!("cargo:rerun-if-changed={path}");
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn test_include(tmpdir: PathBuf) {
        crate::set_other_extensions(vec![".test"]);
        crate::set_nomangle(vec!["example.nomangle.js"]);
        crate::handle_directory(
            PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")),
            &tmpdir,
        );
    }

    fn test_minify(already_minified: &str, filetype: &str) {
        let tmpdir = tempdir().expect("Failed to create test path");
        test_include(tmpdir.path().to_owned());
        let just_minified =
            fs::read_to_string(tmpdir.path().join(format!("assets/example.{filetype}")))
                .expect("Failed to read minified file");
        println!("{just_minified}");
        assert_eq!(already_minified.trim(), just_minified.trim());
    }

    #[test]
    fn js() {
        let minified = include_str!("../assets/example.min.js");
        test_minify(minified, "js");
    }

    #[test]
    fn js_nomangle() {
        let minified = include_str!("../assets/example.min.nomangle.js");
        test_minify(minified, "nomangle.js");
    }

    #[test]
    fn css() {
        let minified = include_str!("../assets/example.min.css");
        test_minify(minified, "css");
    }

    #[test]
    fn html() {
        let minified = include_str!("../assets/example.min.html");
        test_minify(minified, "html");
    }

    #[test]
    fn dirtree() {
        let tmpdir = tempdir().expect("Failed to create test path");
        test_include(tmpdir.path().to_owned());
        let paths = vec![
            "assets",
            "assets/example.css",
            "assets/example.html",
            "assets/example.js",
            "assets/example.min.css",
            "assets/example.min.html",
            "assets/example.min.js",
            "assets/example.min.nomangle.js",
            "assets/example.nomangle.js",
            "assets/test",
            "assets/test/file.test",
        ];
        for path in paths {
            assert!(fs::exists(tmpdir.path().join(path)).unwrap());
        }
        assert!(!fs::exists(tmpdir.path().join("assets/test/file.notincluded")).unwrap());
    }
}
