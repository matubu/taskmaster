use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum TaskmasterDaemonRequest {
	Status,  // get the status of all process
	Reload,  // reload all the configs and restart the processes
	Restart, // restart all the processes

	StartTask(usize),
	StopTask(usize),
	RestartTask(usize),
	InfoTask(usize),    // get the config of a program...

	LoadFile(String),
	UnloadFile(String),
	
	// Logs(String),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum TaskmasterDaemonResult {
	Success,
	Ok(String),
	Raw(String),
	Err(String),
}