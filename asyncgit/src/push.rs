use crate::{
	error::{Error, Result},
	sync::{
		cred::BasicAuthCredential, remotes::push::push,
		remotes::push::ProgressNotification,
	},
	AsyncGitNotification, RemoteProgress, CWD,
};
use crossbeam_channel::{unbounded, Sender};
use std::{
	sync::{Arc, Mutex},
	thread,
};

///
#[derive(Default, Clone, Debug)]
pub struct PushRequest {
	///
	pub remote: String,
	///
	pub branch: String,
	///
	pub force: bool,
	///
	pub delete: bool,
	///
	pub basic_credential: Option<BasicAuthCredential>,
}

#[derive(Default, Clone, Debug)]
struct PushState {
	request: PushRequest,
}

///
pub struct AsyncPush {
	state: Arc<Mutex<Option<PushState>>>,
	last_result: Arc<Mutex<Option<String>>>,
	progress: Arc<Mutex<Option<ProgressNotification>>>,
	sender: Sender<AsyncGitNotification>,
}

impl AsyncPush {
	///
	pub fn new(sender: &Sender<AsyncGitNotification>) -> Self {
		Self {
			state: Arc::new(Mutex::new(None)),
			last_result: Arc::new(Mutex::new(None)),
			progress: Arc::new(Mutex::new(None)),
			sender: sender.clone(),
		}
	}

	///
	pub fn is_pending(&self) -> Result<bool> {
		let state = self.state.lock()?;
		Ok(state.is_some())
	}

	///
	pub fn last_result(&self) -> Result<Option<String>> {
		let res = self.last_result.lock()?;
		Ok(res.clone())
	}

	///
	pub fn progress(&self) -> Result<Option<RemoteProgress>> {
		let res = self.progress.lock()?;
		Ok(res.as_ref().map(|progress| progress.clone().into()))
	}

	///
	pub fn request(&mut self, params: PushRequest) -> Result<()> {
		log::trace!("request");

		if self.is_pending()? {
			return Ok(());
		}

		self.set_request(&params)?;
		RemoteProgress::set_progress(&self.progress, None)?;

		let arc_state = Arc::clone(&self.state);
		let arc_res = Arc::clone(&self.last_result);
		let arc_progress = Arc::clone(&self.progress);
		let sender = self.sender.clone();

		thread::spawn(move || {
			let (progress_sender, receiver) = unbounded();

			let handle = RemoteProgress::spawn_receiver_thread(
				AsyncGitNotification::Push,
				sender.clone(),
				receiver,
				arc_progress,
			);

			let res = push(
				CWD,
				params.remote.as_str(),
				params.branch.as_str(),
				params.force,
				params.delete,
				params.basic_credential.clone(),
				Some(progress_sender.clone()),
			);

			progress_sender
				.send(ProgressNotification::Done)
				.expect("closing send failed");

			handle.join().expect("joining thread failed");

			Self::set_result(&arc_res, res).expect("result error");

			Self::clear_request(&arc_state).expect("clear error");

			sender
				.send(AsyncGitNotification::Push)
				.expect("error sending push");
		});

		Ok(())
	}

	fn set_request(&self, params: &PushRequest) -> Result<()> {
		let mut state = self.state.lock()?;

		if state.is_some() {
			return Err(Error::Generic("pending request".into()));
		}

		*state = Some(PushState {
			request: params.clone(),
		});

		Ok(())
	}

	fn clear_request(
		state: &Arc<Mutex<Option<PushState>>>,
	) -> Result<()> {
		let mut state = state.lock()?;

		*state = None;

		Ok(())
	}

	fn set_result(
		arc_result: &Arc<Mutex<Option<String>>>,
		res: Result<()>,
	) -> Result<()> {
		let mut last_res = arc_result.lock()?;

		*last_res = match res {
			Ok(_) => None,
			Err(e) => {
				log::error!("push error: {}", e);
				Some(e.to_string())
			}
		};

		Ok(())
	}
}
