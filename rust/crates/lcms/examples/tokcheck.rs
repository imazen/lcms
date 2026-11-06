use archmage::SimdToken;
fn main() {
    println!("v3: {:?}", archmage::X64V3Token::summon().is_some());
    println!("v4: {:?}", archmage::X64V4Token::summon().is_some());
    println!("v4x: {:?}", archmage::X64V4xToken::summon().is_some());
    println!("v2: {:?}", archmage::X64V2Token::summon().is_some());
}
