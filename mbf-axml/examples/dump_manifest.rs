//! Debug utility: decodes an AXML file to readable XML.
//! Usage: DUMP_MANIFEST=<input.axml> DUMP_OUT=<output.xml> cargo run -p mbf-axml --example dump_manifest
use std::io::Cursor;

fn main() {
    let input = std::env::var("DUMP_MANIFEST").expect("set DUMP_MANIFEST=<input.axml>");
    let output = std::env::var("DUMP_OUT").expect("set DUMP_OUT=<output.xml>");

    let data = std::fs::read(&input).expect("reading input AXML");
    let mut cursor = Cursor::new(&data);
    let mut reader = mbf_axml::AxmlReader::new(&mut cursor).expect("parsing AXML");

    let mut out = Vec::new();
    {
        let mut writer = xml::EmitterConfig::new()
            .perform_indent(true)
            .create_writer(Cursor::new(&mut out));
        mbf_axml::axml_to_xml(&mut writer, &mut reader).expect("converting AXML to XML");
    }

    std::fs::write(&output, &out).expect("writing output XML");
    println!("wrote {} bytes", out.len());
}
