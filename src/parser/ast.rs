#[derive(Debug, PartialEq)]
pub enum Statement {
    Create {
        table: String,
        columns: Vec<(String, Type)>,
    },
    Insert {
        table: String,
        values: Vec<Value>,
    },
    Select {
        table: String,
    },
}

#[derive(Debug, PartialEq)]
pub enum Type {
    Integer,
    Text,
}

#[derive(Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Str(String),
}
