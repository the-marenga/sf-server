use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

#[derive(Debug, serde::Deserialize)]
#[allow(unused)]
pub struct Request {
    pub req: String,
    rnd: f32,
    c: u32,
}

pub enum ServerResponse {
    Success,
    Data(String),
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("name is not available")]
    InvalidName,
    #[error("character exists")]
    CharacterExists,
    #[error("bad request")]
    BadRequest,
    #[error("wrong pass")]
    WrongPassword,
    #[error("command requires valid session")]
    InvalidAuth,
    #[error("unknown request")]
    UnknownRequest,
    #[error("command missing argument: {0}")]
    MissingArgument(&'static str),
    #[error("internal server error")]
    Internal,
    #[error("need more gold")]
    NotEnoughMoney,
    #[error("still busy")]
    StillBusy,
    #[error("cannot do this right now2")]
    NotRightNow2,
    #[error("internal error")]
    DBError(#[from] libsql::Error),
}

impl From<ServerError> for Response {
    fn from(error: ServerError) -> Response {
        let status = StatusCode::OK;
        match Response::builder()
            .status(status)
            .body(axum::body::Body::new(format!("error:{error}")))
        {
            Ok(resp) => resp,
            Err(_) => status.into_response(),
        }
    }
}

#[derive(Debug, Default)]
pub struct ResponseBuilder {
    resp: String,
    key_start: bool,
}

impl ResponseBuilder {
    pub fn add_key(&mut self, key: &str) -> &mut ResponseBuilder {
        if !self.resp.is_empty() {
            self.resp.push('&')
        }
        self.resp.push_str(key);
        self.resp.push(':');
        self.key_start = true;
        self
    }

    pub fn add_str(&mut self, val: &str) -> &mut ResponseBuilder {
        if !self.key_start {
            self.resp.push('/');
        } else {
            self.key_start = false;
        }
        self.resp.push_str(val);
        self
    }

    pub fn add_val<T: ToString>(&mut self, val: T) -> &mut ResponseBuilder {
        if !self.key_start {
            self.resp.push('/');
        } else {
            self.key_start = false;
        }
        self.resp.push_str(&val.to_string());
        self
    }
    pub fn skip_key(&mut self) -> &mut ResponseBuilder {
        self.key_start = false;
        self.resp.push('&');
        self
    }

    pub fn build(&mut self) -> Result<Response, Response> {
        let mut a = String::new();
        std::mem::swap(&mut a, &mut self.resp);
        Ok(ServerResponse::Data(a).into())
    }
}

impl From<ServerResponse> for Response {
    fn from(resp: ServerResponse) -> Response {
        let status = StatusCode::OK;

        let body = match resp {
            ServerResponse::Success => "Success:".to_string(),
            ServerResponse::Data(data) => data,
        };
        match Response::builder()
            .status(status)
            .body(axum::body::Body::new(body))
        {
            Ok(resp) => resp,
            Err(_) => status.into_response(),
        }
    }
}
