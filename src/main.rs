extern crate clap;
extern crate rspirv_cfg;
use clap::{App, Arg};
use rspirv_cfg::{export_spirv_cfg, SpirvModule};
use std::path::PathBuf;
fn main() {
    let matches = App::new("rspirv-cfg")
        .arg(
            Arg::with_name("file")
                .short("f")
                .long("file")
                .value_name("FILE")
                .help("Path to the .spv file")
                .required(true)
                .takes_value(true),
        )
        .get_matches();
    let file_path = matches.value_of("file").expect("No filename");
    let file_path = PathBuf::from(file_path);
    let module = SpirvModule::load(&file_path);
    export_spirv_cfg(&module);
    //println!("{:#?}", module.names);
}
