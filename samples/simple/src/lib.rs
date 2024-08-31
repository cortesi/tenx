pub struct Example {
    name: String,
}

impl Example {
    pub fn new(name: &str) -> Self {
        Example {
            name: name.to_string(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

pub fn fibonacci(n: u64) -> u64 {
    println!("Calculating fibonacci({})", n);
    0
}

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
