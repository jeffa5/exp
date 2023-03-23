use ::exp::Experiment;
use ::exp::RunConfig;
use pyo3::prelude::*;

// #[pyfunction]
// fn run(py:Python, experiment: &mut Box<dyn Experiment<Configuration=()>>, config: &RunConfig) -> PyResult<&PyAny>{
//     pyo3_asyncio::tokio::future_into_py(py, async {::exp::run(experiment, config).await.unwrap(); Ok(())})
// }

/// A Python module implemented in Rust.
#[pymodule]
fn exp(_py: Python, m: &PyModule) -> PyResult<()> {
    // m.add_function(wrap_pyfunction!(run, m)?)?;
    Ok(())
}
