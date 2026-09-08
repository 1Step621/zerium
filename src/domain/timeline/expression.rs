use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
enum Expression {
    Number(f32),
    Variable(String),
    Negate(Box<Expression>),
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Function {
        function: Function,
        argument: Box<Expression>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Function {
    Sin,
    Cos,
    Tan,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f32),
    Identifier(String),
    Plus,
    Minus,
    Star,
    Slash,
    LeftParen,
    RightParen,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CompiledExpression {
    source: String,
    expression: Expression,
    dependencies: HashSet<String>,
}

impl CompiledExpression {
    pub(super) fn compile(source: String) -> Option<Self> {
        let expression = parse(&source)?;
        let mut dependencies = HashSet::new();
        expression.collect_dependencies(&mut dependencies);
        Some(Self {
            source,
            expression,
            dependencies,
        })
    }

    pub(super) fn source(&self) -> &str {
        &self.source
    }

    pub(super) fn dependencies(&self) -> &HashSet<String> {
        &self.dependencies
    }

    pub(super) fn evaluate(&self, values: &HashMap<String, f32>) -> Option<f32> {
        let value = self.expression.evaluate(values)?;
        value.is_finite().then_some(value)
    }
}

pub(crate) fn valid_variable_name(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
        && !matches!(name, "sin" | "cos" | "tan" | "pi" | "e")
}

pub(crate) fn variable_name_for_label(label: &str) -> Option<String> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed
        .chars()
        .map(|character| {
            if character.is_whitespace() {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    valid_variable_name(&normalized).then_some(normalized)
}

pub(crate) fn rewrite_variables(
    source: &str,
    replacements: &HashMap<String, String>,
) -> Option<String> {
    let mut output = String::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        let character = source.get(index..)?.chars().next()?;
        if character == '_' || character.is_alphabetic() {
            let start = index;
            index += character.len_utf8();
            while index < source.len() {
                let character = source.get(index..)?.chars().next()?;
                if character != '_' && !character.is_alphanumeric() {
                    break;
                }
                index += character.len_utf8();
            }
            let identifier = source.get(start..index)?;
            output.push_str(
                replacements
                    .get(identifier)
                    .map_or(identifier, String::as_str),
            );
        } else {
            output.push(character);
            index += character.len_utf8();
        }
    }
    Some(output)
}

fn parse(source: &str) -> Option<Expression> {
    let tokens = tokenize(source)?;
    let mut parser = Parser {
        tokens,
        position: 0,
    };
    let expression = parser.parse_additive()?;
    (parser.position == parser.tokens.len()).then_some(expression)
}

fn tokenize(source: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < source.len() {
        let character = source.get(index..)?.chars().next()?;
        match character {
            character if character.is_whitespace() => index += character.len_utf8(),
            '+' => {
                tokens.push(Token::Plus);
                index += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                index += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                index += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                index += 1;
            }
            '(' => {
                tokens.push(Token::LeftParen);
                index += 1;
            }
            ')' => {
                tokens.push(Token::RightParen);
                index += 1;
            }
            character if character.is_ascii_digit() || character == '.' => {
                let start = index;
                let mut seen_exponent = false;
                index += 1;
                while index < source.len() {
                    let byte = *source.as_bytes().get(index)?;
                    if byte.is_ascii_digit() || byte == b'.' {
                        index += 1;
                    } else if matches!(byte, b'e' | b'E') && !seen_exponent {
                        seen_exponent = true;
                        index += 1;
                        if index < source.len()
                            && matches!(source.as_bytes().get(index), Some(b'+' | b'-'))
                        {
                            index += 1;
                        }
                    } else {
                        break;
                    }
                }
                let number = source.get(start..index)?.parse::<f32>().ok()?;
                if !number.is_finite() {
                    return None;
                }
                tokens.push(Token::Number(number));
            }
            character if character == '_' || character.is_alphabetic() => {
                let start = index;
                index += character.len_utf8();
                while index < source.len() {
                    let character = source.get(index..)?.chars().next()?;
                    if character != '_' && !character.is_alphanumeric() {
                        break;
                    }
                    index += character.len_utf8();
                }
                tokens.push(Token::Identifier(source.get(start..index)?.to_owned()));
            }
            _ => return None,
        }
    }
    (!tokens.is_empty()).then_some(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn parse_additive(&mut self) -> Option<Expression> {
        let mut expression = self.parse_multiplicative()?;
        loop {
            let operator = match self.tokens.get(self.position) {
                Some(Token::Plus) => BinaryOperator::Add,
                Some(Token::Minus) => BinaryOperator::Subtract,
                _ => break,
            };
            self.position += 1;
            expression = Expression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(self.parse_multiplicative()?),
            };
        }
        Some(expression)
    }

    fn parse_multiplicative(&mut self) -> Option<Expression> {
        let mut expression = self.parse_unary()?;
        loop {
            let operator = match self.tokens.get(self.position) {
                Some(Token::Star) => BinaryOperator::Multiply,
                Some(Token::Slash) => BinaryOperator::Divide,
                _ => break,
            };
            self.position += 1;
            expression = Expression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(self.parse_unary()?),
            };
        }
        Some(expression)
    }

    fn parse_unary(&mut self) -> Option<Expression> {
        match self.tokens.get(self.position) {
            Some(Token::Plus) => {
                self.position += 1;
                self.parse_unary()
            }
            Some(Token::Minus) => {
                self.position += 1;
                Some(Expression::Negate(Box::new(self.parse_unary()?)))
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Option<Expression> {
        match self.tokens.get(self.position)?.clone() {
            Token::Number(value) => {
                self.position += 1;
                Some(Expression::Number(value))
            }
            Token::Identifier(identifier) => {
                self.position += 1;
                if !matches!(self.tokens.get(self.position), Some(Token::LeftParen)) {
                    return Some(Expression::Variable(identifier));
                }
                self.position += 1;
                let function = match identifier.as_str() {
                    "sin" => Function::Sin,
                    "cos" => Function::Cos,
                    "tan" => Function::Tan,
                    _ => return None,
                };
                let argument = self.parse_additive()?;
                if !matches!(self.tokens.get(self.position), Some(Token::RightParen)) {
                    return None;
                }
                self.position += 1;
                Some(Expression::Function {
                    function,
                    argument: Box::new(argument),
                })
            }
            Token::LeftParen => {
                self.position += 1;
                let expression = self.parse_additive()?;
                if !matches!(self.tokens.get(self.position), Some(Token::RightParen)) {
                    return None;
                }
                self.position += 1;
                Some(expression)
            }
            _ => None,
        }
    }
}

impl Expression {
    fn collect_dependencies(&self, dependencies: &mut HashSet<String>) {
        match self {
            Self::Variable(name) if !matches!(name.as_str(), "pi" | "e") => {
                dependencies.insert(name.clone());
            }
            Self::Negate(expression) => expression.collect_dependencies(dependencies),
            Self::Binary { left, right, .. } => {
                left.collect_dependencies(dependencies);
                right.collect_dependencies(dependencies);
            }
            Self::Function { argument, .. } => argument.collect_dependencies(dependencies),
            Self::Number(_) | Self::Variable(_) => {}
        }
    }

    fn evaluate(&self, values: &HashMap<String, f32>) -> Option<f32> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Variable(name) => match name.as_str() {
                "pi" => Some(std::f32::consts::PI),
                "e" => Some(std::f32::consts::E),
                _ => values.get(name).copied(),
            },
            Self::Negate(expression) => Some(-expression.evaluate(values)?),
            Self::Binary {
                operator,
                left,
                right,
            } => {
                let left = left.evaluate(values)?;
                let right = right.evaluate(values)?;
                Some(match operator {
                    BinaryOperator::Add => left + right,
                    BinaryOperator::Subtract => left - right,
                    BinaryOperator::Multiply => left * right,
                    BinaryOperator::Divide => left / right,
                })
            }
            Self::Function { function, argument } => {
                let argument = argument.evaluate(values)?;
                Some(match function {
                    Function::Sin => argument.sin(),
                    Function::Cos => argument.cos(),
                    Function::Tan => argument.tan(),
                })
            }
        }
    }
}
