use aws_runtime_sync::aws::Client;

#[allow(dead_code, non_snake_case, unused_variables, clippy::all)]
mod s3 {
    include!(concat!(env!("OUT_DIR"), "/s3.rs"));
}

fn main() {
    let client = Client::from_env();

    match s3::list_buckets(&client, s3::ListBucketsRequest::default()) {
        Ok(resp) => {
            for bucket in resp.buckets.iter().flat_map(|b| &b.item) {
                println!("  {}", bucket.name.as_deref().unwrap_or("?"));
            }
        }
        Err(e) => eprintln!("Error: {e}"),
    }
}
