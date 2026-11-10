//! End-to-end requests to the gateway through the `ohttp` client.

use hartig_ohttp::{
    Access, Admission, CredentialPolicy, DecapsulationError, Disposition,
    ENCAPSULATED_MESSAGE_BYTES, Gateway, OHTTP_RES_MEDIA_TYPE, PADDED_BHTTP_REQUEST_BYTES,
    PADDED_BHTTP_RESPONSE_BYTES, ServerKeyConfig, message,
};
use http::{Method, Request, Response, StatusCode, header};

fn gateway() -> Gateway {
    Gateway::new(ServerKeyConfig::generate(1).expect("generate key"))
}

fn post(path: &str, body: &[u8]) -> Request<Vec<u8>> {
    Request::builder()
        .method(Method::POST)
        .uri(format!("https://gateway.example{path}"))
        .header("content-type", "application/octet-stream")
        .body(body.to_vec())
        .expect("valid request")
}

/// Client side: encode, pad, encapsulate.
fn encapsulate(
    gateway: &Gateway<impl CredentialPolicy>,
    request: &Request<Vec<u8>>,
) -> (Vec<u8>, ohttp::ClientResponse) {
    let padded = message::pad(
        message::encode_request(request).expect("encode"),
        PADDED_BHTTP_REQUEST_BYTES,
    )
    .expect("fits");
    ohttp::ClientRequest::from_encoded_config_list(gateway.key_config().encoded_ohttp_keys())
        .expect("client from config")
        .encapsulate(&padded)
        .expect("encapsulate")
}

fn decapsulate_response(
    client: ohttp::ClientResponse,
    gateway_response: &Response<Vec<u8>>,
) -> Response<Vec<u8>> {
    assert_eq!(gateway_response.status(), StatusCode::OK);
    assert_eq!(
        gateway_response.headers()[header::CONTENT_TYPE],
        OHTTP_RES_MEDIA_TYPE
    );
    assert_eq!(
        gateway_response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "*"
    );
    message::decode_response(
        &client
            .decapsulate(gateway_response.body())
            .expect("decapsulate"),
    )
    .expect("decode BHTTP response")
}

#[test]
fn encapsulated_messages_have_fixed_size() {
    let gateway = gateway();
    let (encapsulated, client) = encapsulate(&gateway, &post("/mailbox/m1", b"hello"));
    assert_eq!(
        encapsulated.len(),
        ENCAPSULATED_MESSAGE_BYTES,
        "request overhead constant is wrong"
    );

    let gateway_response = gateway.handle(&encapsulated, |request| {
        assert_eq!(request.method(), Method::POST);
        assert_eq!(request.uri().path(), "/mailbox/m1");
        assert_eq!(request.body(), b"hello");
        Response::builder()
            .status(StatusCode::CREATED)
            .body(b"stored".to_vec())
            .expect("valid response")
    });
    assert_eq!(
        gateway_response.body().len(),
        ENCAPSULATED_MESSAGE_BYTES,
        "response overhead constant is wrong"
    );

    let response = decapsulate_response(client, &gateway_response);
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.body(), b"stored");
}

#[test]
fn oversized_response_becomes_encapsulated_500() {
    let gateway = gateway();
    let (encapsulated, client) = encapsulate(&gateway, &post("/big", b""));
    let gateway_response = gateway.handle(&encapsulated, |_| {
        Response::new(vec![0xaa; ENCAPSULATED_MESSAGE_BYTES])
    });
    assert_eq!(gateway_response.body().len(), ENCAPSULATED_MESSAGE_BYTES);
    assert_eq!(
        decapsulate_response(client, &gateway_response).status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[test]
fn largest_fitting_response_fills_the_padding_exactly() {
    let gateway = gateway();
    let (encapsulated, client) = encapsulate(&gateway, &post("/fill", b""));
    let capacity = PADDED_BHTTP_RESPONSE_BYTES;
    // Find the largest body whose BHTTP encoding still fits.
    let fits = |n: usize| {
        message::encode_response(&Response::new(vec![1; n]))
            .map(|e| e.len())
            .unwrap_or(usize::MAX)
            <= capacity
    };
    let max_body = (0..capacity)
        .rev()
        .find(|&n| fits(n))
        .expect("some body fits");
    let gateway_response = gateway.handle(&encapsulated, |_| Response::new(vec![1; max_body]));
    assert_eq!(gateway_response.body().len(), ENCAPSULATED_MESSAGE_BYTES);
    assert_eq!(
        decapsulate_response(client, &gateway_response).body().len(),
        max_body
    );
}

#[test]
fn decapsulation_failures_are_constant_shaped() {
    let gateway = gateway();
    let (good, _) = encapsulate(&gateway, &post("/x", b""));

    let short = good[..good.len() - 1].to_vec();
    let mut long = good.clone();
    long.push(0);
    let mut wrong_key_id = good.clone();
    wrong_key_id[0] ^= 0xff;
    let mut tampered = good.clone();
    if let Some(last) = tampered.last_mut() {
        *last ^= 1;
    }
    let other_key = Gateway::new(ServerKeyConfig::generate(1).expect("generate"));
    let (for_other_key, _) = encapsulate(&other_key, &post("/x", b""));

    let canonical = DecapsulationError.response();
    assert_eq!(canonical.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        canonical.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "*"
    );
    for (name, bad) in [
        ("short", short),
        ("long", long),
        ("wrong key id", wrong_key_id),
        ("tampered", tampered),
        ("other key", for_other_key),
        ("empty", Vec::new()),
    ] {
        assert_eq!(
            gateway.decapsulate(&bad).err(),
            Some(DecapsulationError),
            "{name}"
        );
        let gateway_response = gateway.handle(&bad, |_| unreachable!("service must not run"));
        assert_eq!(gateway_response.status(), canonical.status(), "{name}");
        assert_eq!(gateway_response.headers(), canonical.headers(), "{name}");
        assert_eq!(gateway_response.body(), canonical.body(), "{name}");
    }
}

#[test]
fn malformed_bhttp_request_gets_encapsulated_400() {
    let gateway = gateway();
    let mut padded = message::pad(
        message::encode_request(&post("/x", b"")).expect("encode"),
        PADDED_BHTTP_REQUEST_BYTES,
    )
    .expect("fits");
    if let Some(last) = padded.last_mut() {
        *last = 1; // non-zero padding
    }
    let (encapsulated, client) =
        ohttp::ClientRequest::from_encoded_config_list(gateway.key_config().encoded_ohttp_keys())
            .expect("client")
            .encapsulate(&padded)
            .expect("encapsulate");
    let gateway_response = gateway.handle(&encapsulated, |_| unreachable!("service must not run"));
    assert_eq!(
        decapsulate_response(client, &gateway_response).status(),
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn expect_100_continue_gets_encapsulated_417() {
    let gateway = gateway();
    let mut request = post("/mailbox/m1", b"x");
    request.headers_mut().insert(
        header::EXPECT,
        http::HeaderValue::from_static("100-Continue"),
    );
    let (encapsulated, client) = encapsulate(&gateway, &request);
    let gateway_response = gateway.handle(&encapsulated, |_| unreachable!("service must not run"));
    assert_eq!(
        decapsulate_response(client, &gateway_response).status(),
        StatusCode::EXPECTATION_FAILED
    );
}

struct DenyWrites;

impl CredentialPolicy for DenyWrites {
    fn admit(&self, access: Access, _request: &Request<Vec<u8>>) -> Admission {
        match access {
            Access::Read => Admission::Admit,
            Access::Write => Admission::Deny(StatusCode::UNAUTHORIZED),
        }
    }
}

#[test]
fn credential_slot_runs_on_reads_and_writes() {
    let gateway = Gateway::with_policy(ServerKeyConfig::generate(1).expect("generate"), DenyWrites);

    let (write, client) = encapsulate(&gateway, &post("/mailbox/m1", b"x"));
    let request = gateway.decapsulate(&write).expect("decapsulates");
    let Disposition::Refused(refusal) = request.disposition else {
        panic!("write should be refused");
    };
    let encapsulated_response = request
        .response_context
        .encapsulate(refusal)
        .expect("encapsulate");
    assert_eq!(encapsulated_response.len(), ENCAPSULATED_MESSAGE_BYTES);
    let response = message::decode_response(
        &client
            .decapsulate(&encapsulated_response)
            .expect("decapsulate"),
    )
    .expect("decode");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let read = Request::builder()
        .method(Method::GET)
        .uri("https://gateway.example/mailbox/m1")
        .body(Vec::new())
        .expect("valid");
    let (read, _) = encapsulate(&gateway, &read);
    assert!(matches!(
        gateway
            .decapsulate(&read)
            .expect("decapsulates")
            .disposition,
        Disposition::Admitted(_)
    ));
}
