use actix_web::{HttpResponse, Responder};

#[derive(Debug, serde::Deserialize)]
#[allow(unused)]
pub struct Request {
    pub req: String,
    rnd: f32,
    c: u32,
}

pub enum Response {
    Success,
    Data(String),
    Error(Error),
}

pub enum Error {
    InvalidName,
    CharacterExists,
    BadRequest,
    UnknownRequest,
    WrongPassword,
    InvalidAuth,
    MissingArgument(&'static str),
    Internal,
    NotEnoughMoney,
    StillBusy,
}

impl Error {
    pub fn resp(self) -> Response {
        Response::Error(self)
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

    pub fn build(&mut self) -> Response {
        let mut a = String::new();
        std::mem::swap(&mut a, &mut self.resp);
        Response::Data(a)
    }
}

impl Error {
    pub fn error_str(&self) -> String {
        match self {
            Error::InvalidName => "name is not available",
            Error::CharacterExists => "character exists",
            Error::BadRequest => "bad request",
            Error::WrongPassword => "wrong pass",
            Error::InvalidAuth => "command requires valid session",
            Error::UnknownRequest => "unknown request",
            Error::MissingArgument(name) => {
                return format!("command missing argument: {name}")
            }
            Error::Internal => "internal server error",
            Error::NotEnoughMoney => "need more gold",
            Error::StillBusy => "still busy",
        }
        .to_string()
    }
}

impl Responder for Response {
    type Body = actix_web::body::BoxBody;

    fn respond_to(
        self,
        _req: &actix_web::HttpRequest,
    ) -> HttpResponse<Self::Body> {
        let body = match self {
            Response::Success => "Success:".to_string(),
            Response::Data(d) => d,
            Response::Error(e) => format!("error:{}", e.error_str()),
        };

        HttpResponse::Ok().body(body)
    }
}
