//! Build deterministic host and property interfaces from validated core schemas.

use std::collections::BTreeMap;

use zerium_core::plugin::ShaderKind;

use crate::CAPABILITY_SAMPLER_BINDING;
use contract::{ShaderContract, property_module};

pub(crate) mod contract;
pub(crate) mod output;
mod property;

pub(crate) const GENERATED_DIR: &str = "generated";
pub(crate) const MANIFEST_FINGERPRINT: &str = "manifest.fingerprint";
const UTIL_INTERFACE: &str = include_str!("wesl/util.wesl");

pub(crate) fn generated_files(
    contracts: &BTreeMap<String, ShaderContract<'_>>,
) -> BTreeMap<String, String> {
    let mut files = BTreeMap::from([
        ("host/util.wesl".to_owned(), UTIL_INTERFACE.to_owned()),
        (
            "host/_context.wesl".to_owned(),
            include_str!("wesl/_context.wesl").to_owned(),
        ),
    ]);
    for kind in ShaderKind::ALL {
        files.insert(
            format!("host/{}.wesl", kind.module_name()),
            host_interface(kind).to_owned(),
        );
        files.insert(
            format!("host/{}.wesl", kind.internal_module_name()),
            internal_interface(kind),
        );
    }
    for (entity, contract) in contracts {
        for &kind in &contract.kinds {
            let source = property::interface(contract.layout, kind)
                + "\n"
                + &capability_interface(&contract.input_ids);
            let module = property_module(entity, kind);
            // Omit the generated root when writing the interface. Asset names
            // remain platform-independent so fingerprints match across OSes.
            let file = format!("{}.wesl", module.components[1..].join("/"));
            files.insert(file, source);
        }
    }
    for source in files.values_mut() {
        *source = source.replace("\r\n", "\n");
    }
    files
}

pub(crate) fn fingerprint(files: &BTreeMap<String, String>) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for (name, source) in files {
        // File boundaries are explicit; normalize content exactly as write() does.
        for byte in name
            .bytes()
            .chain([0])
            .chain(source.trim_end().bytes())
            .chain([b'\n', 0])
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

fn capability_interface(input_ids: &[String]) -> String {
    if input_ids.is_empty() {
        return String::new();
    }
    let mut source = String::new();
    for (binding, id) in input_ids.iter().enumerate() {
        source.push_str(&format!(
            "@group(1) @binding({binding})\nvar {id}: texture_2d<f32>;\n\n"
        ));
    }
    source.push_str(&format!(
        "@group(1) @binding({})\nvar capability_sampler: sampler;\n",
        CAPABILITY_SAMPLER_BINDING,
    ));
    source
}

fn host_interface(kind: ShaderKind) -> &'static str {
    match kind {
        ShaderKind::Item => include_str!("wesl/item.wesl"),
        ShaderKind::Effect => include_str!("wesl/effect.wesl"),
        ShaderKind::Compute => include_str!("wesl/compute.wesl"),
        ShaderKind::Temporal => include_str!("wesl/temporal.wesl"),
    }
}

fn internal_interface(kind: ShaderKind) -> String {
    let kind_source = match kind {
        ShaderKind::Item => include_str!("wesl/_item.wesl"),
        ShaderKind::Effect => include_str!("wesl/_effect.wesl"),
        ShaderKind::Compute => include_str!("wesl/_compute.wesl"),
        ShaderKind::Temporal => include_str!("wesl/_temporal.wesl"),
    };
    format!("{kind_source}\n{}", include_str!("wesl/raw_props.wesl"))
}
