//! Debug utility: decodes an AXML file and prints every element with its attributes
//! (name, resource id, value type, data) for structural comparison.
//! Usage: DUMP_MANIFEST=<input.axml> cargo run -p mbf-axml --example dump_attrs
use std::io::Cursor;

fn main() {
    let input = std::env::var("DUMP_MANIFEST").expect("set DUMP_MANIFEST=<input.axml>");
    let data = std::fs::read(&input).expect("reading input AXML");
    let mut cursor = Cursor::new(&data);
    let mut reader = mbf_axml::AxmlReader::new(&mut cursor).expect("parsing AXML");

    let mut depth = 0usize;
    while let Ok(Some(event)) = reader.read_next_event() {
        match event {
            mbf_axml::Event::StartElement { attributes, name, .. } => {
                let indent = "  ".repeat(depth);
                println!("{indent}<{name}>");
                for attr in &attributes {
                    let value = match &attr.value {
                        mbf_axml::AttributeValue::String(s) => format!("STRING({s})"),
                        mbf_axml::AttributeValue::Boolean(b) => format!("BOOL({b})"),
                        mbf_axml::AttributeValue::Integer(i) => format!("INT({i})"),
                        mbf_axml::AttributeValue::Reference(r) => format!("REF({r:#x})"),
                        mbf_axml::AttributeValue::Float(f) => format!("FLOAT({f})"),
                        mbf_axml::AttributeValue::Dimension(d) => format!("DIM({d})"),
                    };
                    let res_id = match attr.resource_id {
                        Some(id) => format!("{id:#010x}"),
                        None => "none".to_owned(),
                    };
                    let attr_name = &attr.name;
                    println!("{indent}  [{res_id}] {attr_name} = {value}");
                }
                depth += 1;
            }
            mbf_axml::Event::EndElement { .. } => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
}
