pub enum TaskmasterDaemonRequest {
	Status,  // get the status of all process
	Reload,  // reload all the configs and restart the processes
	Restart, // restart all the processes

	Start(String),
	Stop(String),
	Restart(String),

	Load(String),
	Unload(String),
	Reload(String),
	
	// Logs(String),
}

pub enum TaskmasterDaemonResult {
	Success(String),
	Fail(String),
}