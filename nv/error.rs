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
    /// The destination is not a worker this route has a link for.
    ///
    /// Its own outcome rather than a clamp or a folded `Full`, because a route has a finite table
    /// and a frame delivered to whichever worker happens to sit at the end of it is a fault that
    /// cannot be seen from either end — the sender is told it succeeded and the receiver has no
    /// reason to doubt the frame. Refusing is the only answer that leaves both ends able to tell.
    NoSuchRank,
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
