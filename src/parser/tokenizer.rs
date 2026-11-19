#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Keyword(Keyword),
    Ident(String),
    Int(i64),
    Str(String),
    LParen,
    RParen,
    Comma,
    Semicolon,
    Star,
    Eq,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Keyword {
    Select,
    From,
    Insert,
    Into,
    Values,
    Create,
    Table,
    Integer,
    Text,
}

impl Keyword {
    fn from_word(word: &str) -> Option<Keyword> {
        match word.to_ascii_uppercase().as_str() {
            "SELECT" => Some(Keyword::Select),
            "FROM" => Some(Keyword::From),
            "INSERT" => Some(Keyword::Insert),
            "INTO" => Some(Keyword::Into),
            "VALUES" => Some(Keyword::Values),
            "CREATE" => Some(Keyword::Create),
            "TABLE" => Some(Keyword::Table),
            "INTEGER" | "INT" => Some(Keyword::Integer),
            "TEXT" => Some(Keyword::Text),
            _ => None,
        }
    }
}

pub fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c.is_alphabetic() || c == '_' {
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_alphanumeric() || c == '_' {
                    word.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(match Keyword::from_word(&word) {
                Some(kw) => Token::Keyword(kw),
                None => Token::Ident(word),
            });
        } else if c.is_ascii_digit() {
            let mut num = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    num.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            let n = num.parse().map_err(|_| format!("number too large: {num}"))?;
            tokens.push(Token::Int(n));
        } else if c == '\'' {
            chars.next(); // opening quote
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some('\'') => break,
                    Some(c) => s.push(c),
                    None => return Err("unterminated string".into()),
                }
            }
            tokens.push(Token::Str(s));
        } else {
            tokens.push(match c {
                '(' => Token::LParen,
                ')' => Token::RParen,
                ',' => Token::Comma,
                ';' => Token::Semicolon,
                '*' => Token::Star,
                '=' => Token::Eq,
                _ => return Err(format!("unexpected character '{c}'")),
            });
            chars.next();
        }
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_star() {
        assert_eq!(
            tokenize("select * FROM users;").unwrap(),
            vec![
                Token::Keyword(Keyword::Select),
                Token::Star,
                Token::Keyword(Keyword::From),
                Token::Ident("users".into()),
                Token::Semicolon,
            ]
        );
    }

    #[test]
    fn literals() {
        assert_eq!(
            tokenize("(42, 'hi there')").unwrap(),
            vec![
                Token::LParen,
                Token::Int(42),
                Token::Comma,
                Token::Str("hi there".into()),
                Token::RParen,
            ]
        );
    }

    #[test]
    fn unterminated_string_errors() {
        assert!(tokenize("'oops").is_err());
    }
}
