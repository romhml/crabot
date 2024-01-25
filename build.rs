use std::error::Error;
use std::process::{exit, Command};

fn clean_build() -> Result<(), Box<dyn Error>> {
    Command::new("sh")
        .arg("-c")
        .arg("rm -rf ./assets/dist")
        .spawn()
        .expect("Failed to clean build");

    Ok(())
}
fn build_tailwind() -> Result<(), Box<dyn Error>> {
    Command::new("sh")
        .arg("-c")
        .arg("bunx tailwind -i assets/tailwind.css -o assets/dist/main.css")
        .spawn()
        .expect("Failed to build tailwind.");

    Ok(())
}

fn build_lib() -> Result<(), Box<dyn Error>> {
    Command::new("sh")
        .arg("-c")
        .arg("bun build js/htmx --outdir assets/dist/")
        .spawn()
        .expect("Failed to build lib.");

    Ok(())
}

// fn build_components() -> Result<(), Box<dyn Error>> {
//     Command::new("sh")
//         .arg("-c")
//         .arg("bun build js/components --outdir assets/dist/")
//         .spawn()
//         .expect("Failed to build lib.");
//
//     Ok(())
// }

fn main() {
    if let Err(err) = clean_build() {
        eprintln!("{}", err);
        exit(1);
    }

    println!("cargo:rerun-if-changed=assets/tailwind.css");
    println!("cargo:rerun-if-changed=templates/*.html");
    println!("cargo:rerun-if-changed=templates/pages/*.html");
    println!("cargo:rerun-if-changed=templates/elements/*.html");
    if let Err(err) = build_tailwind() {
        eprintln!("{}", err);
        exit(1);
    }

    println!("cargo:rerun-if-changed=js/*.js");
    if let Err(err) = build_lib() {
        eprintln!("{}", err);
        exit(1);
    }

    // println!("cargo:rerun-if-changed=js/*.ts");
    // if let Err(err) = build_components() {
    //     eprintln!("{}", err);
    //     exit(1);
    // }

    // if let Err(err) = build_components() {
    //     eprintln!("{}", err);
    //     exit(1);
    // }
}
