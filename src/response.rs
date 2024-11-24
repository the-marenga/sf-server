use std::fmt::Write;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

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
    #[error("unknown request: {0}")]
    UnknownRequest(Box<str>),
    #[error("command missing argument: {0}")]
    MissingArgument(&'static str),
    #[error("need more gold")]
    NotEnoughMoney,
    #[error("still busy")]
    StillBusy,
    #[error("cannot do this right now2")]
    NotRightNow2,
    #[error("internal server error: {0}")]
    DBError(#[from] sqlx::Error),
    #[error("internal server error")]
    Internal,
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

    pub fn add_val<T: std::fmt::Display>(
        &mut self,
        val: T,
    ) -> &mut ResponseBuilder {
        if !self.key_start {
            self.resp.push('/');
        } else {
            self.key_start = false;
        }
        self.resp.write_fmt(format_args!("{val}")).unwrap();
        self
    }
    pub fn skip_key(&mut self) -> &mut ResponseBuilder {
        self.key_start = false;
        self.resp.push('&');
        self
    }

    pub fn build<T>(&mut self) -> Result<ServerResponse, T> {
        let mut a = String::new();
        std::mem::swap(&mut a, &mut self.resp);
        Ok(ServerResponse::Data(a))
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
