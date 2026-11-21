//! SQL text -> tokens -> `Statement`.
//!
//! Grammar so far:
//!   SELECT * FROM <table>
//!   INSERT INTO <table> VALUES ( <value> , ... )
//!   CREATE TABLE <table> ( <column> <type> , ... )
//! Each may end with an optional `;`.

pub mod ast;
pub mod tokenizer;

use ast::{Statement, Type, Value};
use tokenizer::{Keyword, Token};

/// Parse one SQL statement.
pub fn parse(input: &str) -> Result<Statement, String> {
    let tokens = tokenizer::tokenize(input)?;
    Parser { tokens, pos: 0 }.parse_statement()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn parse_statement(&mut self) -> Result<Statement, String> {
        let statement = match self.peek() {
            Some(Token::Keyword(Keyword::Select)) => self.parse_select()?,
            Some(Token::Keyword(Keyword::Insert)) => self.parse_insert()?,
            Some(Token::Keyword(Keyword::Create)) => self.parse_create()?,
            Some(t) => return Err(format!("unexpected {t:?} at start of statement")),
            None => return Err("empty statement".into()),
        };

        if self.peek() == Some(&Token::Semicolon) {
            self.next();
        }
        if let Some(t) = self.peek() {
            return Err(format!("unexpected {t:?} after end of statement"));
        }
        Ok(statement)
    }

    // SELECT * FROM <table>
    fn parse_select(&mut self) -> Result<Statement, String> {
        self.expect(Token::Keyword(Keyword::Select))?;
        self.expect(Token::Star)?;
        self.expect(Token::Keyword(Keyword::From))?;
        let table = self.ident()?;
        Ok(Statement::Select { table })
    }

    // INSERT INTO <table> VALUES ( <value> , ... )
    fn parse_insert(&mut self) -> Result<Statement, String> {
        self.expect(Token::Keyword(Keyword::Insert))?;
        self.expect(Token::Keyword(Keyword::Into))?;
        let table = self.ident()?;
        self.expect(Token::Keyword(Keyword::Values))?;
        let values = self.comma_list(Self::value)?;
        Ok(Statement::Insert { table, values })
    }

    // CREATE TABLE <table> ( <column> <type> , ... )
    fn parse_create(&mut self) -> Result<Statement, String> {
        self.expect(Token::Keyword(Keyword::Create))?;
        self.expect(Token::Keyword(Keyword::Table))?;
        let table = self.ident()?;
        let columns = self.comma_list(|p| Ok((p.ident()?, p.column_type()?)))?;
        Ok(Statement::Create { table, columns })
    }

    // ---- pieces ----------------------------------------------------------

    /// `( item , item , ... )` with at least one item.
    fn comma_list<T>(
        &mut self,
        mut item: impl FnMut(&mut Self) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        self.expect(Token::LParen)?;
        let mut items = vec![item(self)?];
        while self.peek() == Some(&Token::Comma) {
            self.next();
            items.push(item(self)?);
        }
        self.expect(Token::RParen)?;
        Ok(items)
    }

    fn ident(&mut self) -> Result<String, String> {
        match self.next() {
            Some(Token::Ident(name)) => Ok(name),
            other => Err(format!("expected a name, found {other:?}")),
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.next() {
            Some(Token::Int(n)) => Ok(Value::Int(n)),
            Some(Token::Str(s)) => Ok(Value::Str(s)),
            other => Err(format!("expected a value, found {other:?}")),
        }
    }

    fn column_type(&mut self) -> Result<Type, String> {
        match self.next() {
            Some(Token::Keyword(Keyword::Integer)) => Ok(Type::Integer),
            Some(Token::Keyword(Keyword::Text)) => Ok(Type::Text),
            other => Err(format!("expected INTEGER or TEXT, found {other:?}")),
        }
    }

    // ---- token helpers ---------------------------------------------------

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        token
    }

    fn expect(&mut self, expected: Token) -> Result<(), String> {
        match self.next() {
            Some(t) if t == expected => Ok(()),
            other => Err(format!("expected {expected:?}, found {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select() {
        assert_eq!(
            parse("SELECT * FROM users;").unwrap(),
            Statement::Select {
                table: "users".into()
            }
        );
    }

    #[test]
    fn insert() {
        assert_eq!(
            parse("insert into users values (1, 'alice')").unwrap(),
            Statement::Insert {
                table: "users".into(),
                values: vec![Value::Int(1), Value::Str("alice".into())],
            }
        );
    }

    #[test]
    fn create() {
        assert_eq!(
            parse("CREATE TABLE users (id INTEGER, name TEXT);").unwrap(),
            Statement::Create {
                table: "users".into(),
                columns: vec![("id".into(), Type::Integer), ("name".into(), Type::Text)],
            }
        );
    }

    #[test]
    fn errors() {
        assert!(parse("").is_err());
        assert!(parse("SELECT FROM users").is_err());
        assert!(parse("SELECT * FROM users extra").is_err());
        assert!(parse("INSERT INTO users VALUES ()").is_err());
    }
}
