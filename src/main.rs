fn main() {
    let mut tokenizer = recount::tokenizer::Tokenizer::new("2023-02-01 1.2334343".to_string());
    let _token = tokenizer.next_token();
}
