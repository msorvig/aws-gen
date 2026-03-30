use aws_runtime_async::aws::Client;

#[allow(dead_code, non_snake_case, unused_variables, clippy::all)]
mod ec2 {
    include!(concat!(env!("OUT_DIR"), "/ec2.rs"));
}

#[allow(dead_code, non_snake_case, unused_variables, clippy::all)]
mod ssm {
    include!(concat!(env!("OUT_DIR"), "/ssm.rs"));
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let client = Client::from_env();

    // EC2: list instances
    println!("=== EC2 Instances ===");
    match ec2::describe_instances(&client, ec2::DescribeInstancesRequest::default()).await {
        Ok(resp) => {
            let count: usize = resp.reservations.iter()
                .flat_map(|r| &r.item)
                .flat_map(|r| r.instances.iter().flat_map(|s| &s.item))
                .count();
            println!("  {count} instance(s)");
        }
        Err(e) => eprintln!("  Error: {e}"),
    }

    // SSM: get a parameter
    println!("\n=== SSM Parameters ===");
    let input = ssm::GetParametersRequest {
        names: ssm::ParameterNameList {
            item: vec!["/aws/service/global-infrastructure/current-region".into()],
        },
        ..Default::default()
    };
    match ssm::get_parameters(&client, input).await {
        Ok(resp) => {
            for p in resp.parameters.iter().flat_map(|l| &l.item) {
                let name = p.name.as_deref().unwrap_or("?");
                let value = p.value.as_deref().unwrap_or("?");
                println!("  {name} = {value}");
            }
        }
        Err(e) => eprintln!("  Error: {e}"),
    }
}
