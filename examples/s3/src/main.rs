use aws_runtime_async::aws::Client;

#[allow(dead_code, non_snake_case, unused_variables, clippy::all)]
mod s3 {
    include!(concat!(env!("OUT_DIR"), "/s3.rs"));
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let client = Client::from_env();

    // List buckets
    println!("=== Buckets ===");
    match s3::list_buckets(&client, s3::ListBucketsRequest::default()).await {
        Ok(resp) => {
            for bucket in resp.buckets.iter().flat_map(|b| &b.item) {
                let name = bucket.name.as_deref().unwrap_or("?");
                let region = bucket.bucket_region.as_deref().unwrap_or("?");
                println!("  {name}  ({region})");
            }
        }
        Err(e) => eprintln!("Error listing buckets: {e}"),
    }

    // List objects in first bucket (if any)
    println!("\n=== First bucket contents (max 10) ===");
    let buckets_resp = s3::list_buckets(&client, s3::ListBucketsRequest::default()).await;
    if let Ok(resp) = buckets_resp {
        if let Some(first) = resp.buckets.as_ref().and_then(|b| b.item.first()) {
            let bucket_name = first.name.clone().unwrap_or_default();
            let input = s3::ListObjectsV2Request {
                bucket: bucket_name.clone(),
                max_keys: Some(10),
                ..Default::default()
            };
            match s3::list_objects_v2(&client, input).await {
                Ok(objects) => {
                    for obj in objects.contents.iter().flat_map(|c| &c.item) {
                        let key = obj.key.as_deref().unwrap_or("?");
                        let size = obj.size.unwrap_or(0);
                        println!("  {key}  ({size} bytes)");
                    }
                }
                Err(e) => eprintln!("Error listing objects in {bucket_name}: {e}"),
            }
        }
    }
}
