//! What `#[component]`'s builders are made of: each property of a
//! component's builder is [`Unset`] until the template gives it, then
//! [`Set`]. Only the generated code names them.

/// A property the template has not given (yet): its default, if it has one.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Unset;

/// A property the template gave.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct Set<T>(pub T);
