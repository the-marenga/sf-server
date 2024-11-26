use crate::ServerError;

pub trait OptionGet<V> {
    fn get(self, name: &'static str) -> Result<V, ServerError>;
}

impl<T> OptionGet<T> for Option<T> {
    fn get(self, name: &'static str) -> Result<T, ServerError> {
        self.ok_or_else(|| ServerError::MissingArgument(name))
    }
}
