use clickhouse::Client;

#[derive(Debug, thiserror::Error)]
#[error("ClickHouse rejected migration statement {statement_index}")]
pub(crate) struct SqlExecutionError {
    pub statement_index: usize,
    #[source]
    pub source: clickhouse::error::Error,
}

pub(crate) async fn run_sql(client: &Client, sql: &str) -> Result<(), SqlExecutionError> {
    for (index, statement) in split_sql_statements(sql).into_iter().enumerate() {
        client
            .query(statement.trim())
            .execute()
            .await
            .map_err(|source| SqlExecutionError {
                statement_index: index + 1,
                source,
            })?;
    }
    Ok(())
}

fn split_sql_statements(sql: &str) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        SingleQuote,
        DoubleQuote,
        Backtick,
        LineComment,
        BlockComment,
    }

    let characters = sql.chars().collect::<Vec<_>>();
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut state = State::Normal;
    let mut index = 0_usize;

    while index < characters.len() {
        let character = characters[index];
        let next = characters.get(index + 1).copied();
        match state {
            State::Normal => match (character, next) {
                ('-', Some('-')) => {
                    current.push_str("--");
                    state = State::LineComment;
                    index += 1;
                }
                ('/', Some('*')) => {
                    current.push_str("/*");
                    state = State::BlockComment;
                    index += 1;
                }
                ('\'', _) => {
                    current.push(character);
                    state = State::SingleQuote;
                }
                ('"', _) => {
                    current.push(character);
                    state = State::DoubleQuote;
                }
                ('`', _) => {
                    current.push(character);
                    state = State::Backtick;
                }
                (';', _) => {
                    if has_executable_sql(&current) {
                        statements.push(std::mem::take(&mut current));
                    } else {
                        current.clear();
                    }
                }
                _ => current.push(character),
            },
            State::SingleQuote => {
                current.push(character);
                if character == '\'' {
                    if next == Some('\'') {
                        current.push('\'');
                        index += 1;
                    } else if !is_backslash_escaped(&characters, index) {
                        state = State::Normal;
                    }
                }
            }
            State::DoubleQuote => {
                current.push(character);
                if character == '"' && !is_backslash_escaped(&characters, index) {
                    state = State::Normal;
                }
            }
            State::Backtick => {
                current.push(character);
                if character == '`' {
                    state = State::Normal;
                }
            }
            State::LineComment => {
                current.push(character);
                if character == '\n' {
                    state = State::Normal;
                }
            }
            State::BlockComment => {
                current.push(character);
                if character == '*' && next == Some('/') {
                    current.push('/');
                    index += 1;
                    state = State::Normal;
                }
            }
        }
        index += 1;
    }

    if has_executable_sql(&current) {
        statements.push(current);
    }
    statements
}

fn is_backslash_escaped(characters: &[char], index: usize) -> bool {
    let mut backslashes = 0_usize;
    let mut cursor = index;
    while cursor > 0 && characters[cursor - 1] == '\\' {
        backslashes += 1;
        cursor -= 1;
    }
    backslashes % 2 == 1
}

fn has_executable_sql(statement: &str) -> bool {
    statement.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !line.starts_with("--") && !line.starts_with("/*")
    })
}

#[cfg(test)]
mod tests {
    use super::split_sql_statements;

    #[test]
    fn preserves_semicolons_inside_literals_and_comments() {
        let statements = split_sql_statements(
            r#"
            -- comment; still comment
            INSERT INTO events VALUES ('a;b');
            /* block; comment */
            SELECT "x;y";
            "#,
        );
        assert_eq!(statements.len(), 2);
        assert!(statements[0].contains("'a;b'"));
        assert!(statements[1].contains("\"x;y\""));
    }
}
