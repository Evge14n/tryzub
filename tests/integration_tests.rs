use tryzub::lexer::tokenize;
use tryzub::parser::parse;
use tryzub::vm::execute;

#[test]
fn test_hello_world() {
    let source = r#"
функція головна() {
    друк("Привіт, світ!")
}
"#;
    
    let tokens = tokenize(source).expect("Tokenization failed");
    let ast = parse(tokens).expect("Parsing failed");
    
    // VM виконання
    assert!(execute(ast, vec![]).is_ok());
}

#[test]
fn test_arithmetic() {
    let source = r#"
функція головна() {
    змінна а = 10
    змінна б = 20
    змінна с = а + б
    друк(с)
}
"#;
    
    let tokens = tokenize(source).expect("Tokenization failed");
    let ast = parse(tokens).expect("Parsing failed");
    
    assert!(execute(ast, vec![]).is_ok());
}

#[test]
fn test_if_statement() {
    let source = r#"
функція головна() {
    змінна x = 42
    
    якщо (x > 40) {
        друк("x більше 40")
    } інакше {
        друк("x менше або дорівнює 40")
    }
}
"#;
    
    let tokens = tokenize(source).expect("Tokenization failed");
    let ast = parse(tokens).expect("Parsing failed");
    
    assert!(execute(ast, vec![]).is_ok());
}

#[test]
fn test_for_loop() {
    let source = r#"
функція головна() {
    для (i від 1 до 5) {
        друк(i)
    }
}
"#;
    
    let tokens = tokenize(source).expect("Tokenization failed");
    let ast = parse(tokens).expect("Parsing failed");
    
    assert!(execute(ast, vec![]).is_ok());
}

#[test]
fn test_function_call() {
    let source = r#"
функція квадрат(x: цл32) -> цл32 {
    повернути x * x
}

функція головна() {
    змінна результат = квадрат(5)
    друк(результат)
}
"#;
    
    let tokens = tokenize(source).expect("Tokenization failed");
    let ast = parse(tokens).expect("Parsing failed");
    
    assert!(execute(ast, vec![]).is_ok());
}

#[test]
fn test_string_operations() {
    let source = r#"
функція головна() {
    змінна привіт = "Привіт"
    змінна світ = "світ"
    змінна повідомлення = привіт + ", " + світ + "!"
    друк(повідомлення)
}
"#;
    
    let tokens = tokenize(source).expect("Tokenization failed");
    let ast = parse(tokens).expect("Parsing failed");
    
    assert!(execute(ast, vec![]).is_ok());
}

#[test]
fn test_array_operations() {
    let source = r#"
функція головна() {
    змінна числа = [1, 2, 3, 4, 5]
    
    для (число в числа) {
        друк(число)
    }
    
    друк(числа[2])
}
"#;
    
    let tokens = tokenize(source).expect("Tokenization failed");
    let ast = parse(tokens).expect("Parsing failed");
    
    assert!(execute(ast, vec![]).is_ok());
}

#[test]
fn test_error_handling() {
    let source = r#"
функція ділення(а: дрб64, б: дрб64) -> дрб64 {
    якщо (б == 0.0) {
        помилка("Ділення на нуль!")
    }
    повернути а / б
}

функція головна() {
    спробувати {
        змінна результат = ділення(10.0, 0.0)
        друк(результат)
    } зловити (е) {
        друк("Помилка: " + повідомлення_помилки(е))
    }
}
"#;
    
    let tokens = tokenize(source).expect("Tokenization failed");
    let ast = parse(tokens).expect("Parsing failed");
    
    assert!(execute(ast, vec![]).is_ok());
}
