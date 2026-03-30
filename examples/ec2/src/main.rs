use aws_runtime_async::aws::Client;

#[allow(dead_code, non_snake_case, unused_variables, clippy::all)]
mod ec2 {
    include!(concat!(env!("OUT_DIR"), "/ec2.rs"));
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let client = Client::from_env();

    match ec2::describe_instances(&client, ec2::DescribeInstancesRequest::default()).await {
        Ok(resp) => {
            for res in resp.reservations.iter().flat_map(|r| &r.item) {
                for inst in res.instances.iter().flat_map(|s| &s.item) {
                    let id = inst.instance_id.as_deref().unwrap_or("?");
                    let state = inst.state.as_ref()
                        .and_then(|s| s.name.as_ref())
                        .map(|n| format!("{:?}", n))
                        .unwrap_or_else(|| "?".into());
                    let itype = inst.instance_type.as_ref()
                        .map(|t| format!("{:?}", t))
                        .unwrap_or_else(|| "?".into());
                    println!("{id}  {state}  {itype}");
                }
            }
        }
        Err(e) => eprintln!("Error: {e}"),
    }
}
