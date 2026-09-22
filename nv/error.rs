#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The layout is invalid.
    Layout(LayoutError),
    /// The send buffer is too large for the layout.
    Send(SendError),
    /// The receive buffer is too small for the message.
    Recv(RecvError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    InvalidDepth,
    TooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendError {
    Full,
    TooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecvError {
    Empty,
    TooSmall { needed: u32 },
}

impl From<LayoutError> for Error {
    fn from(e: LayoutError) -> Self {
        Error::Layout(e)
    }
}

impl From<SendError> for Error {
    fn from(e: SendError) -> Self {
        Error::Send(e)
    }
}

impl From<RecvError> for Error {
    fn from(e: RecvError) -> Self {
        Error::Recv(e)
    }
}
