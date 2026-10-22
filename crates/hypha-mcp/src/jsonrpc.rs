//! JSON-RPC 2.0 envelopes, as MCP narrows them: one message per HTTP request (no
//! batches), request IDs that are strings or integers and never `null`, params that
//! are an object.

use serde_json::{Map, Value, json};

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
/// The HTTP headers don't match the body, or a required one is missing.
pub const HEADER_MISMATCH: i64 = -32020;
pub const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// A message the client sent.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Request {
        id: Value,
        method: String,
        params: Map<String, Value>,
    },
    Notification {
        method: String,
    },
    /// A response to a request of ours. The server sends none, so there's nothing to
    /// match it with; it's accepted and dropped.
    Response,
}

/// A JSON-RPC error, with the request's ID when it could be read.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub id: Option<Value>,
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(id: Option<Value>, code: i64, message: impl Into<String>) -> Self {
        Self {
            id,
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// The response object. Without an ID (the request's couldn't be read) the `id`
    /// member is left out, as MCP's transport asks.
    pub fn to_json(&self) -> Value {
        let mut error = Map::new();
        error.insert("code".into(), json!(self.code));
        error.insert("message".into(), json!(self.message));
        if let Some(data) = &self.data {
            error.insert("data".into(), data.clone());
        }
        let mut response = Map::new();
        response.insert("jsonrpc".into(), json!("2.0"));
        if let Some(id) = &self.id {
            response.insert("id".into(), id.clone());
        }
        response.insert("error".into(), Value::Object(error));
        Value::Object(response)
    }
}

pub fn result(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// Reads one message from the request body.
pub fn parse(body: &[u8]) -> Result<Message, RpcError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| RpcError::new(None, PARSE_ERROR, format!("Parse error: {e}")))?;
    let object = match value {
        Value::Object(object) => object,
        Value::Array(_) => {
            return Err(RpcError::new(
                None,
                INVALID_REQUEST,
                "Invalid Request: batches are not supported; send one message per request",
            ));
        }
        _ => {
            return Err(RpcError::new(
                None,
                INVALID_REQUEST,
                "Invalid Request: expected a JSON-RPC object",
            ));
        }
    };
    if object.get("jsonrpc") != Some(&json!("2.0")) {
        return Err(RpcError::new(
            readable_id(&object),
            INVALID_REQUEST,
            "Invalid Request: \"jsonrpc\" must be \"2.0\"",
        ));
    }
    let id = match object.get("id") {
        None => None,
        Some(id) if is_valid_id(id) => Some(id.clone()),
        Some(_) => {
            return Err(RpcError::new(
                None,
                INVALID_REQUEST,
                "Invalid Request: \"id\" must be a string or an integer",
            ));
        }
    };
    match object.get("method") {
        Some(Value::String(method)) => {
            let params = match object.get("params") {
                None => Map::new(),
                Some(Value::Object(params)) => params.clone(),
                Some(_) => {
                    let error = RpcError::new(
                        id.clone(),
                        INVALID_PARAMS,
                        "Invalid params: \"params\" must be an object",
                    );
                    // A notification gets no response, not even an error; the
                    // transport still refuses it.
                    return Err(error);
                }
            };
            Ok(match id {
                Some(id) => Message::Request {
                    id,
                    method: method.clone(),
                    params,
                },
                None => Message::Notification {
                    method: method.clone(),
                },
            })
        }
        Some(_) => Err(RpcError::new(
            id,
            INVALID_REQUEST,
            "Invalid Request: \"method\" must be a string",
        )),
        None if id.is_some() && (object.contains_key("result") || object.contains_key("error")) => {
            Ok(Message::Response)
        }
        None => Err(RpcError::new(
            id,
            INVALID_REQUEST,
            "Invalid Request: no \"method\"",
        )),
    }
}

fn is_valid_id(id: &Value) -> bool {
    match id {
        Value::String(_) => true,
        Value::Number(n) => n.is_i64() || n.is_u64(),
        _ => false,
    }
}

fn readable_id(object: &Map<String, Value>) -> Option<Value> {
    object.get("id").filter(|id| is_valid_id(id)).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code_of(body: &str) -> i64 {
        match parse(body.as_bytes()) {
            Err(error) => error.code,
            Ok(message) => panic!("expected an error, got {message:?}"),
        }
    }

    #[test]
    fn reads_a_request() {
        assert_eq!(
            parse(br#"{"jsonrpc":"2.0","id":7,"method":"tools/list","params":{"cursor":"x"}}"#),
            Ok(Message::Request {
                id: json!(7),
                method: "tools/list".into(),
                params: json!({"cursor": "x"})
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            })
        );
    }

    #[test]
    fn a_request_without_params_has_empty_params() {
        assert_eq!(
            parse(br#"{"jsonrpc":"2.0","id":"a","method":"ping"}"#),
            Ok(Message::Request {
                id: json!("a"),
                method: "ping".into(),
                params: Map::new(),
            })
        );
    }

    #[test]
    fn a_message_without_id_is_a_notification() {
        assert_eq!(
            parse(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            Ok(Message::Notification {
                method: "notifications/initialized".into()
            })
        );
    }

    #[test]
    fn a_result_or_error_without_method_is_a_response() {
        assert_eq!(
            parse(br#"{"jsonrpc":"2.0","id":1,"result":{}}"#),
            Ok(Message::Response)
        );
        assert_eq!(
            parse(br#"{"jsonrpc":"2.0","id":1,"error":{"code":1,"message":"x"}}"#),
            Ok(Message::Response)
        );
    }

    #[test]
    fn broken_json_is_a_parse_error() {
        assert_eq!(code_of("{\"jsonrpc\":"), PARSE_ERROR);
    }

    #[test]
    fn batches_are_refused() {
        assert_eq!(
            code_of(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#),
            INVALID_REQUEST
        );
    }

    #[test]
    fn a_non_object_is_refused() {
        assert_eq!(code_of("42"), INVALID_REQUEST);
    }

    #[test]
    fn the_version_must_be_2_0() {
        assert_eq!(
            code_of(r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#),
            INVALID_REQUEST
        );
        assert_eq!(code_of(r#"{"id":1,"method":"ping"}"#), INVALID_REQUEST);
    }

    #[test]
    fn null_float_and_boolean_ids_are_refused() {
        for id in ["null", "1.5", "true"] {
            let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ping"}}"#);
            assert_eq!(code_of(&body), INVALID_REQUEST, "id {id}");
        }
    }

    #[test]
    fn params_must_be_an_object() {
        let error = parse(br#"{"jsonrpc":"2.0","id":3,"method":"ping","params":[1]}"#);
        assert_eq!(
            error,
            Err(RpcError::new(
                Some(json!(3)),
                INVALID_PARAMS,
                "Invalid params: \"params\" must be an object"
            ))
        );
    }

    #[test]
    fn a_message_without_method_or_result_is_refused() {
        assert_eq!(code_of(r#"{"jsonrpc":"2.0","id":1}"#), INVALID_REQUEST);
    }

    #[test]
    fn an_error_without_id_leaves_the_member_out() {
        let json = RpcError::new(None, PARSE_ERROR, "Parse error").to_json();
        assert_eq!(
            json,
            json!({"jsonrpc": "2.0", "error": {"code": -32700, "message": "Parse error"}})
        );
    }

    #[test]
    fn an_error_carries_its_id_and_data() {
        let json = RpcError::new(Some(json!("x")), INVALID_PARAMS, "bad")
            .with_data(json!({"k": 1}))
            .to_json();
        assert_eq!(
            json,
            json!({"jsonrpc": "2.0", "id": "x",
                   "error": {"code": -32602, "message": "bad", "data": {"k": 1}}})
        );
    }
}
