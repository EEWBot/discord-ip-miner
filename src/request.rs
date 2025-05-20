pub type Job = crate::request::Request;
pub type JobSender = async_channel::Sender<Job>;
pub type JobReceiver = async_channel::Receiver<Job>;

#[derive(Clone, Debug)]
pub struct Request {
    pub target: url::Url,
}
