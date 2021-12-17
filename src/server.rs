use tokio::io::{AsyncRead, AsyncWrite};

pub async fn accept<T>(_stream: T)
where
    T: AsyncWrite + AsyncRead + Unpin,
{
    unimplemented!()
}
