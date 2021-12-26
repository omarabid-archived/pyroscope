// Copyright 2021 Developers of Pyroscope.

// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0>. This file may not be copied, modified, or distributed
// except according to those terms.

extern crate pyroscope;

use pyroscope::{PyroscopeAgent, Result};

fn fibonacci(n: u64) -> u64 {
    match n {
        0 | 1 => 1,
        n => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

fn main() -> Result<()>{
    let mut agent =
        PyroscopeAgent::builder("http://localhost:4040", "fibonacci")
            .frequency(100)
            .tags(
                &[
                    ("TagA", "ValueA"),
                    ("TagB", "ValueB"),
                ]
            )
            .build()
            ?;

    agent.start()?;
    for s in &[1, 10, 40, 50] {
        let result = fibonacci(44);
        println!("fibonacci({}) -> {}", *s, result);
    }
    agent.stop()?;

    for s in &[1, 10, 40, 50] {
        let result = fibonacci(44);
        println!("fibonacci({}) -> {}", *s, result);
    }

    agent.start()?;
    for s in &[1, 10, 40, 50] {
        let result = fibonacci(44);
        println!("fibonacci({}) -> {}", *s, result);
    }
    let backend = std::sync::Arc::clone(&agent.backend);
    let report = backend.lock().unwrap().report()?;
    println!("{}", std::str::from_utf8(&report).unwrap()); 
    agent.stop()?;

    Ok(())
}
