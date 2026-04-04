use std::fs;
use std::path::Path;

const BOTOCORE: &str =
    "https://raw.githubusercontent.com/boto/botocore/develop/botocore/data";

const SERVICES: &[(&str, &str)] = &[
    ("ec2",    "2016-11-15"),
    ("s3",     "2006-03-01"),
    ("ssm",    "2014-11-06"),
    ("iam",    "2010-05-08"),
    ("lambda", "2015-03-31"),
    ("sts",    "2011-06-15"),
];

fn fetch(service: &str, date: &str, specs_dir: &Path) {
    let out = specs_dir.join(format!("{service}.json"));
    if out.exists() {
        return;
    }

    let url = format!("{BOTOCORE}/{service}/{date}/service-2.json");
    println!("cargo::warning=specs: fetching {service} {date} ...");

    let resp = ureq::get(&url).call()
        .unwrap_or_else(|e| panic!("failed to fetch {url}: {e}"));

    let body = resp.into_body().read_to_vec()
        .unwrap_or_else(|e| panic!("failed to read response for {service}: {e}"));

    fs::write(&out, &body)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", out.display()));

    println!("cargo::warning=specs: wrote {} ({} bytes)", out.display(), body.len());
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let specs_dir = Path::new(&manifest_dir).join("specs");
    fs::create_dir_all(&specs_dir).unwrap();

    for &(service, date) in SERVICES {
        fetch(service, date, &specs_dir);
    }

    // Only re-run if a spec file is missing or build.rs itself changes
    println!("cargo::rerun-if-changed=build.rs");
    for &(service, _) in SERVICES {
        println!("cargo::rerun-if-changed=specs/{service}.json");
    }
}
