use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum TaskmasterDaemonRequest {
	Status,  // get the status of all process
	Reload,  // reload all the configs and restart the processes
	Restart, // restart all the processes

	StartProgram(String),
	StopProgram(String),
	RestartProgram(String),

	LoadFile(String),
	UnloadFile(String),
	ReloadFile(String),
	
	// Logs(String),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum TaskmasterDaemonResult {
	Success(String),
	Fail(String),
}