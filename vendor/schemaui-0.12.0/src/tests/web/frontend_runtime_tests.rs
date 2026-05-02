use std::net::{IpAddr, SocketAddr, TcpListener};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;
use ureq::Agent;

use crate::web::session::ServeOptions;
use crate::{FrontendOptions, SchemaUI};

fn reserve_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
    let port = listener.local_addr().expect("listener addr").port();
    drop(listener);
    port
}

fn test_http_agent() -> Agent {
    Agent::new_with_config(
        Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(2)))
            .proxy(None)
            .build(),
    )
}

fn wait_until_ready(agent: &Agent, base_url: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let session_url = format!("{base_url}/api/session");

    loop {
        let outcome = match agent.get(&session_url).call() {
            Ok(response) if response.status().as_u16() == 200 => return,
            Ok(response) => format!("unexpected status: {}", response.status()),
            Err(err) => err.to_string(),
        };

        assert!(
            Instant::now() < deadline,
            "web session did not become ready at {base_url}; last outcome: {}",
            outcome
        );
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn web_frontend_exit_does_not_panic_when_runtime_is_dropped() {
    let port = reserve_port();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let base_url = format!("http://{addr}");
    let expected = json!({ "name": "alice" });
    let schema = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    let handle = thread::spawn(move || {
        SchemaUI::from_schema(schema).run(FrontendOptions::Web(ServeOptions {
            host: IpAddr::from([127, 0, 0, 1]),
            port,
        }))
    });

    let agent = test_http_agent();
    wait_until_ready(&agent, &base_url);

    let response = agent
        .post(format!("{base_url}/api/exit"))
        .content_type("application/json")
        .send(
            serde_json::to_string(&json!({
                "data": expected.clone(),
                "commit": true
            }))
            .expect("serialize exit payload"),
        )
        .expect("post exit request");
    assert_eq!(response.status().as_u16(), 200);

    let result = handle
        .join()
        .expect("web frontend thread should not panic")
        .expect("web frontend should return the committed payload");
    assert_eq!(result, expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn web_frontend_async_runner_works_inside_existing_runtime() {
    let port = reserve_port();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let base_url = format!("http://{addr}");
    let expected = json!({ "name": "bob" });
    let schema = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    let task = tokio::spawn(async move {
        SchemaUI::from_schema(schema)
            .run_web_async(ServeOptions {
                host: IpAddr::from([127, 0, 0, 1]),
                port,
            })
            .await
    });

    let wait_url = base_url.clone();
    tokio::task::spawn_blocking(move || {
        let agent = test_http_agent();
        wait_until_ready(&agent, &wait_url);
    })
    .await
    .expect("wait task should not panic");

    let exit_url = format!("{base_url}/api/exit");
    let exit_payload = json!({
        "data": expected.clone(),
        "commit": true
    });
    let response = tokio::task::spawn_blocking(move || {
        test_http_agent()
            .post(exit_url)
            .content_type("application/json")
            .send(serde_json::to_string(&exit_payload).expect("serialize exit payload"))
    })
    .await
    .expect("exit task should not panic")
    .expect("post exit request");
    assert_eq!(response.status().as_u16(), 200);

    let result = task
        .await
        .expect("web frontend task should not panic")
        .expect("web frontend should return the committed payload");
    assert_eq!(result, expected);
}
