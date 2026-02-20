use self :: Token :: * ; use crate :: cow_rc_str :: CowRcStr ; use crate :: parser :: ParserState ; use matches :: matches ; use std :: char ; use std :: i32 ; use std :: ops :: Range ; # [doc = " One of the pieces the CSS input is broken into."] # [doc = ""] # [doc = " Some components use `Cow` in order to borrow from the original input string"] # [doc = " and avoid allocating/copying when possible."] # [derive (PartialEq , Debug , Clone)] pub enum Token < 'a > {
# [doc = " A [`<ident-token>`](https://drafts.csswg.org/css-syntax/#ident-token-diagram)"] Ident (CowRcStr < 'a >) , # [doc = " A [`<at-keyword-token>`](https://drafts.csswg.org/css-syntax/#at-keyword-token-diagram)"] # [doc = ""] # [doc = " The value does not include the `@` marker."] AtKeyword (CowRcStr < 'a >) , # [doc = " A [`<hash-token>`](https://drafts.csswg.org/css-syntax/#hash-token-diagram) with the type flag set to \"unrestricted\""] # [doc = ""] # [doc = " The value does not include the `#` marker."] Hash (CowRcStr < 'a >) , # [doc = " A [`<hash-token>`](https://drafts.csswg.org/css-syntax/#hash-token-diagram) with the type flag set to \"id\""] # [doc = ""] # [doc = " The value does not include the `#` marker."] IDHash (CowRcStr < 'a >) , # [doc = " A [`<string-token>`](https://drafts.csswg.org/css-syntax/#string-token-diagram)"] # [doc = ""] # [doc = " The value does not include the quotes."] QuotedString (CowRcStr < 'a >) , # [doc = " A [`<url-token>`](https://drafts.csswg.org/css-syntax/#url-token-diagram)"] # [doc = ""] # [doc = " The value does not include the `url(` `)` markers.  Note that `url( <string-token> )` is represented by a"] # [doc = " `Function` token."] UnquotedUrl (CowRcStr < 'a >) , # [doc = " A `<delim-token>`"] Delim (char) , # [doc = " A [`<number-token>`](https://drafts.csswg.org/css-syntax/#number-token-diagram)"] Number {
# [doc = " Whether the number had a `+` or `-` sign."] # [doc = ""] # [doc = " This is used is some cases like the <An+B> micro syntax. (See the `parse_nth` function.)"] has_sign : bool , # [doc = " The value as a float"] value : f32 , # [doc = " If the origin source did not include a fractional part, the value as an integer."] int_value : Option < i32 > ,
} , # [doc = " A [`<percentage-token>`](https://drafts.csswg.org/css-syntax/#percentage-token-diagram)"] Percentage {
# [doc = " Whether the number had a `+` or `-` sign."] has_sign : bool , # [doc = " The value as a float, divided by 100 so that the nominal range is 0.0 to 1.0."] unit_value : f32 , # [doc = " If the origin source did not include a fractional part, the value as an integer."] # [doc = " It is **not** divided by 100."] int_value : Option < i32 > ,
} , # [doc = " A [`<dimension-token>`](https://drafts.csswg.org/css-syntax/#dimension-token-diagram)"] Dimension {
# [doc = " Whether the number had a `+` or `-` sign."] # [doc = ""] # [doc = " This is used is some cases like the <An+B> micro syntax. (See the `parse_nth` function.)"] has_sign : bool , # [doc = " The value as a float"] value : f32 , # [doc = " If the origin source did not include a fractional part, the value as an integer."] int_value : Option < i32 > , # [doc = " The unit, e.g. \"px\" in `12px`"] unit : CowRcStr < 'a > ,
} , # [doc = " A [`<whitespace-token>`](https://drafts.csswg.org/css-syntax/#whitespace-token-diagram)"] WhiteSpace (& 'a str) , # [doc = " A comment."] # [doc = ""] # [doc = " The CSS Syntax spec does not generate tokens for comments,"] # [doc = " But we do, because we can (borrowed &str makes it cheap)."] # [doc = ""] # [doc = " The value does not include the `/*` `*/` markers."] Comment (& 'a str) , # [doc = " A `:` `<colon-token>`"] Colon , # [doc = " A `;` `<semicolon-token>`"] Semicolon , # [doc = " A `,` `<comma-token>`"] Comma , # [doc = " A `~=` [`<include-match-token>`](https://drafts.csswg.org/css-syntax/#include-match-token-diagram)"] IncludeMatch , # [doc = " A `|=` [`<dash-match-token>`](https://drafts.csswg.org/css-syntax/#dash-match-token-diagram)"] DashMatch , # [doc = " A `^=` [`<prefix-match-token>`](https://drafts.csswg.org/css-syntax/#prefix-match-token-diagram)"] PrefixMatch , # [doc = " A `$=` [`<suffix-match-token>`](https://drafts.csswg.org/css-syntax/#suffix-match-token-diagram)"] SuffixMatch , # [doc = " A `*=` [`<substring-match-token>`](https://drafts.csswg.org/css-syntax/#substring-match-token-diagram)"] SubstringMatch , # [doc = " A `<!--` [`<CDO-token>`](https://drafts.csswg.org/css-syntax/#CDO-token-diagram)"] CDO , # [doc = " A `-->` [`<CDC-token>`](https://drafts.csswg.org/css-syntax/#CDC-token-diagram)"] CDC , # [doc = " A [`<function-token>`](https://drafts.csswg.org/css-syntax/#function-token-diagram)"] # [doc = ""] # [doc = " The value (name) does not include the `(` marker."] Function (CowRcStr < 'a >) , # [doc = " A `<(-token>`"] ParenthesisBlock , # [doc = " A `<[-token>`"] SquareBracketBlock , # [doc = " A `<{-token>`"] CurlyBracketBlock , # [doc = " A `<bad-url-token>`"] # [doc = ""] # [doc = " This token always indicates a parse error."] BadUrl (CowRcStr < 'a >) , # [doc = " A `<bad-string-token>`"] # [doc = ""] # [doc = " This token always indicates a parse error."] BadString (CowRcStr < 'a >) , # [doc = " A `<)-token>`"] # [doc = ""] # [doc = " When obtained from one of the `Parser::next*` methods,"] # [doc = " this token is always unmatched and indicates a parse error."] CloseParenthesis , # [doc = " A `<]-token>`"] # [doc = ""] # [doc = " When obtained from one of the `Parser::next*` methods,"] # [doc = " this token is always unmatched and indicates a parse error."] CloseSquareBracket , # [doc = " A `<}-token>`"] # [doc = ""] # [doc = " When obtained from one of the `Parser::next*` methods,"] # [doc = " this token is always unmatched and indicates a parse error."] CloseCurlyBracket ,
} impl < 'a > Token < 'a > {
# [doc = " Return whether this token represents a parse error."] # [doc = ""] # [doc = " `BadUrl` and `BadString` are tokenizer-level parse errors."] # [doc = ""] # [doc = " `CloseParenthesis`, `CloseSquareBracket`, and `CloseCurlyBracket` are *unmatched*"] # [doc = " and therefore parse errors when returned by one of the `Parser::next*` methods."] pub fn is_parse_error (& self) -> bool {
matches ! (* self , BadUrl (_) | BadString (_) | CloseParenthesis | CloseSquareBracket | CloseCurlyBracket)
}
} # [derive (Clone)] pub struct Tokenizer < 'a > {
input : & 'a str , # [doc = " Counted in bytes, not code points. From 0."] position : usize , # [doc = " The position at the start of the current line; but adjusted to"] # [doc = " ensure that computing the column will give the result in units"] # [doc = " of UTF-16 characters."] current_line_start_position : usize , current_line_number : u32 , var_or_env_functions : SeenStatus , source_map_url : Option < & 'a str > , source_url : Option < & 'a str > ,
} # [derive (Copy , Clone , PartialEq , Eq)] enum SeenStatus {
DontCare , LookingForThem , SeenAtLeastOne ,
} impl < 'a > Tokenizer < 'a > {
# [inline] pub fn new (input : & str) -> Tokenizer {
Tokenizer :: with_first_line_number (input , 0)
} # [inline] pub fn with_first_line_number (input : & str , first_line_number : u32) -> Tokenizer {
Tokenizer {
input : input , position : 0 , current_line_start_position : 0 , current_line_number : first_line_number , var_or_env_functions : SeenStatus :: DontCare , source_map_url : None , source_url : None ,
}
} # [inline] pub fn look_for_var_or_env_functions (& mut self) {
self . var_or_env_functions = SeenStatus :: LookingForThem ;
} # [inline] pub fn seen_var_or_env_functions (& mut self) -> bool {
let seen = self . var_or_env_functions == SeenStatus :: SeenAtLeastOne ; self . var_or_env_functions = SeenStatus :: DontCare ; seen
} # [inline] pub fn see_function (& mut self , name : & str) {
if self . var_or_env_functions == SeenStatus :: LookingForThem {
if name . eq_ignore_ascii_case ("var") || name . eq_ignore_ascii_case ("env") {
self . var_or_env_functions = SeenStatus :: SeenAtLeastOne ;
}
}
} # [inline] pub fn next (& mut self) -> Result < Token < 'a > , () > {
next_token (self)
} # [inline] pub fn position (& self) -> SourcePosition {
SourcePosition (self . position)
} # [inline] pub fn current_source_location (& self) -> SourceLocation {
SourceLocation {
line : self . current_line_number , column : (self . position - self . current_line_start_position + 1) as u32 ,
}
} # [inline] pub fn current_source_map_url (& self) -> Option < & 'a str > {
self . source_map_url
} # [inline] pub fn current_source_url (& self) -> Option < & 'a str > {
self . source_url
} # [inline] pub fn state (& self) -> ParserState {
ParserState {
position : self . position , current_line_start_position : self . current_line_start_position , current_line_number : self . current_line_number , at_start_of : None ,
}
} # [inline] pub fn reset (& mut self , state : & ParserState) {
self . position = state . position ; self . current_line_start_position = state . current_line_start_position ; self . current_line_number = state . current_line_number ;
} # [inline] pub fn slice_from (& self , start_pos : SourcePosition) -> & 'a str {
& self . input [start_pos . 0 .. self . position]
} # [inline] pub fn slice (& self , range : Range < SourcePosition >) -> & 'a str {
& self . input [range . start . 0 .. range . end . 0]
} pub fn current_source_line (& self) -> & 'a str {
let current = self . position ; let start = self . input [0 .. current] . rfind (| c | matches ! (c , '\r' | '\n' | '\x0C')) . map_or (0 , | start | start + 1) ; let end = self . input [current ..] . find (| c | matches ! (c , '\r' | '\n' | '\x0C')) . map_or (self . input . len () , | end | current + end) ; & self . input [start .. end]
} # [inline] pub fn next_byte (& self) -> Option < u8 > {
if self . is_eof () {
None
} else {
Some (self . input . as_bytes () [self . position])
}
} # [inline] fn is_eof (& self) -> bool {
! self . has_at_least (0)
} # [inline] fn has_at_least (& self , n : usize) -> bool {
self . position + n < self . input . len ()
} # [inline] pub fn advance (& mut self , n : usize) {
if cfg ! (debug_assertions) {
for i in 0 .. n {
let b = self . byte_at (i) ; debug_assert ! (b . is_ascii () || (b & 0xF0 != 0xF0 && b & 0xC0 != 0x80)) ; debug_assert ! (b != b'\r' && b != b'\n' && b != b'\x0C') ;
}
} self . position += n
} # [inline] fn next_byte_unchecked (& self) -> u8 {
self . byte_at (0)
} # [inline] fn byte_at (& self , offset : usize) -> u8 {
self . input . as_bytes () [self . position + offset]
} # [inline] fn consume_4byte_intro (& mut self) {
debug_assert ! (self . next_byte_unchecked () & 0xF0 == 0xF0) ; self . current_line_start_position = self . current_line_start_position . wrapping_sub (1) ; self . position += 1 ;
} # [inline] fn consume_continuation_byte (& mut self) {
debug_assert ! (self . next_byte_unchecked () & 0xC0 == 0x80) ; self . current_line_start_position = self . current_line_start_position . wrapping_add (1) ; self . position += 1 ;
} # [inline (never)] fn consume_known_byte (& mut self , byte : u8) {
debug_assert ! (byte != b'\r' && byte != b'\n' && byte != b'\x0C') ; self . position += 1 ; if byte & 0xF0 == 0xF0 {
self . current_line_start_position = self . current_line_start_position . wrapping_sub (1) ;
} else if byte & 0xC0 == 0x80 {
self . current_line_start_position = self . current_line_start_position . wrapping_add (1) ;
}
} # [inline] fn next_char (& self) -> char {
self . input [self . position ..] . chars () . next () . unwrap ()
} # [inline] fn consume_newline (& mut self) {
let byte = self . next_byte_unchecked () ; debug_assert ! (byte == b'\r' || byte == b'\n' || byte == b'\x0C') ; self . position += 1 ; if byte == b'\r' && self . next_byte () == Some (b'\n') {
self . position += 1 ;
} self . current_line_start_position = self . position ; self . current_line_number += 1 ;
} # [inline] fn has_newline_at (& self , offset : usize) -> bool {
self . position + offset < self . input . len () && matches ! (self . byte_at (offset) , b'\n' | b'\r' | b'\x0C')
} # [inline] fn consume_char (& mut self) -> char {
let c = self . next_char () ; let len_utf8 = c . len_utf8 () ; self . position += len_utf8 ; self . current_line_start_position = self . current_line_start_position . wrapping_add (len_utf8 - c . len_utf16 ()) ; c
} # [inline] fn starts_with (& self , needle : & [u8]) -> bool {
self . input . as_bytes () [self . position ..] . starts_with (needle)
} pub fn skip_whitespace (& mut self) {
while ! self . is_eof () {
{
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize
} static __CASES : [Case ; 256] = [Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case1 , Case :: Case2 , Case :: Case4 , Case :: Case2 , Case :: Case2 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case1 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case3 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4] ; match __CASES [self . next_byte_unchecked () as usize] {
Case :: Case1 => {
{
self . advance (1)
}
} , Case :: Case2 => {
{
self . consume_newline () ;
}
} , Case :: Case3 => {
{
if self . starts_with (b"/*") {
consume_comment (self) ;
} else {
return
}
}
} , Case :: Case4 => {
{
return
}
}
}
}
}
} pub fn skip_cdc_and_cdo (& mut self) {
while ! self . is_eof () {
{
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize , Case5 = 5isize , Case6 = 6isize
} static __CASES : [Case ; 256] = [Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case1 , Case :: Case2 , Case :: Case6 , Case :: Case2 , Case :: Case2 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case1 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case5 , Case :: Case6 , Case :: Case3 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case4 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6] ; match __CASES [self . next_byte_unchecked () as usize] {
Case :: Case1 => {
{
self . advance (1)
}
} , Case :: Case2 => {
{
self . consume_newline () ;
}
} , Case :: Case3 => {
{
if self . starts_with (b"/*") {
consume_comment (self) ;
} else {
return
}
}
} , Case :: Case4 => {
{
if self . starts_with (b"<!--") {
self . advance (4)
} else {
return
}
}
} , Case :: Case5 => {
{
if self . starts_with (b"-->") {
self . advance (3)
} else {
return
}
}
} , Case :: Case6 => {
{
return
}
}
}
}
}
}
} # [doc = " A position from the start of the input, counted in UTF-8 bytes."] # [derive (PartialEq , Eq , PartialOrd , Ord , Debug , Clone , Copy)] pub struct SourcePosition (pub (crate) usize) ; impl SourcePosition {
# [doc = " Returns the current byte index in the original input."] # [inline] pub fn byte_index (& self) -> usize {
self . 0
}
} # [doc = " The line and column number for a given position within the input."] # [derive (PartialEq , Eq , Debug , Clone , Copy)] pub struct SourceLocation {
# [doc = " The line number, starting at 0 for the first line, unless `with_first_line_number` was used."] pub line : u32 , # [doc = " The column number within a line, starting at 1 for first the character of the line."] # [doc = " Column numbers are counted in UTF-16 code units."] pub column : u32 ,
} fn next_token < 'a > (tokenizer : & mut Tokenizer < 'a >) -> Result < Token < 'a > , () > {
if tokenizer . is_eof () {
return Err (()) ;
} let b = tokenizer . next_byte_unchecked () ; let token = {
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize , Case5 = 5isize , Case6 = 6isize , Case7 = 7isize , Case8 = 8isize , Case9 = 9isize , Case10 = 10isize , Case11 = 11isize , Case12 = 12isize , Case13 = 13isize , Case14 = 14isize , Case15 = 15isize , Case16 = 16isize , Case17 = 17isize , Case18 = 18isize , Case19 = 19isize , Case20 = 20isize , Case21 = 21isize , Case22 = 22isize , Case23 = 23isize , Case24 = 24isize , Case25 = 25isize , Case26 = 26isize , Case27 = 27isize , Case28 = 28isize , Case29 = 29isize
} static __CASES : [Case ; 256] = [Case :: Case20 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case1 , Case :: Case2 , Case :: Case29 , Case :: Case2 , Case :: Case2 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case1 , Case :: Case29 , Case :: Case3 , Case :: Case4 , Case :: Case5 , Case :: Case29 , Case :: Case29 , Case :: Case6 , Case :: Case7 , Case :: Case8 , Case :: Case9 , Case :: Case10 , Case :: Case11 , Case :: Case12 , Case :: Case13 , Case :: Case14 , Case :: Case15 , Case :: Case15 , Case :: Case15 , Case :: Case15 , Case :: Case15 , Case :: Case15 , Case :: Case15 , Case :: Case15 , Case :: Case15 , Case :: Case15 , Case :: Case16 , Case :: Case17 , Case :: Case18 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case19 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case21 , Case :: Case22 , Case :: Case23 , Case :: Case24 , Case :: Case20 , Case :: Case29 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case20 , Case :: Case25 , Case :: Case26 , Case :: Case27 , Case :: Case28 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29 , Case :: Case29] ; match __CASES [b as usize] {
Case :: Case1 => {
{
consume_whitespace (tokenizer , false)
}
} , Case :: Case2 => {
{
consume_whitespace (tokenizer , true)
}
} , Case :: Case3 => {
{
consume_string (tokenizer , false)
}
} , Case :: Case4 => {
{
tokenizer . advance (1) ; if is_ident_start (tokenizer) {
IDHash (consume_name (tokenizer))
} else if ! tokenizer . is_eof () && match tokenizer . next_byte_unchecked () {
b'0' ..= b'9' | b'-' => true , _ => false ,
} {
Hash (consume_name (tokenizer))
} else {
Delim ('#')
}
}
} , Case :: Case5 => {
{
if tokenizer . starts_with (b"$=") {
tokenizer . advance (2) ; SuffixMatch
} else {
tokenizer . advance (1) ; Delim ('$')
}
}
} , Case :: Case6 => {
{
consume_string (tokenizer , true)
}
} , Case :: Case7 => {
{
tokenizer . advance (1) ; ParenthesisBlock
}
} , Case :: Case8 => {
{
tokenizer . advance (1) ; CloseParenthesis
}
} , Case :: Case9 => {
{
if tokenizer . starts_with (b"*=") {
tokenizer . advance (2) ; SubstringMatch
} else {
tokenizer . advance (1) ; Delim ('*')
}
}
} , Case :: Case10 => {
{
if (tokenizer . has_at_least (1) && matches ! (tokenizer . byte_at (1) , b'0' ..= b'9')) || (tokenizer . has_at_least (2) && tokenizer . byte_at (1) == b'.' && matches ! (tokenizer . byte_at (2) , b'0' ..= b'9')) {
consume_numeric (tokenizer)
} else {
tokenizer . advance (1) ; Delim ('+')
}
}
} , Case :: Case11 => {
{
tokenizer . advance (1) ; Comma
}
} , Case :: Case12 => {
{
if (tokenizer . has_at_least (1) && matches ! (tokenizer . byte_at (1) , b'0' ..= b'9')) || (tokenizer . has_at_least (2) && tokenizer . byte_at (1) == b'.' && matches ! (tokenizer . byte_at (2) , b'0' ..= b'9')) {
consume_numeric (tokenizer)
} else if tokenizer . starts_with (b"-->") {
tokenizer . advance (3) ; CDC
} else if is_ident_start (tokenizer) {
consume_ident_like (tokenizer)
} else {
tokenizer . advance (1) ; Delim ('-')
}
}
} , Case :: Case13 => {
{
if tokenizer . has_at_least (1) && matches ! (tokenizer . byte_at (1) , b'0' ..= b'9') {
consume_numeric (tokenizer)
} else {
tokenizer . advance (1) ; Delim ('.')
}
}
} , Case :: Case14 => {
{
if tokenizer . starts_with (b"/*") {
Comment (consume_comment (tokenizer))
} else {
tokenizer . advance (1) ; Delim ('/')
}
}
} , Case :: Case15 => {
{
consume_numeric (tokenizer)
}
} , Case :: Case16 => {
{
tokenizer . advance (1) ; Colon
}
} , Case :: Case17 => {
{
tokenizer . advance (1) ; Semicolon
}
} , Case :: Case18 => {
{
if tokenizer . starts_with (b"<!--") {
tokenizer . advance (4) ; CDO
} else {
tokenizer . advance (1) ; Delim ('<')
}
}
} , Case :: Case19 => {
{
tokenizer . advance (1) ; if is_ident_start (tokenizer) {
AtKeyword (consume_name (tokenizer))
} else {
Delim ('@')
}
}
} , Case :: Case20 => {
{
consume_ident_like (tokenizer)
}
} , Case :: Case21 => {
{
tokenizer . advance (1) ; SquareBracketBlock
}
} , Case :: Case22 => {
{
if ! tokenizer . has_newline_at (1) {
consume_ident_like (tokenizer)
} else {
tokenizer . advance (1) ; Delim ('\\')
}
}
} , Case :: Case23 => {
{
tokenizer . advance (1) ; CloseSquareBracket
}
} , Case :: Case24 => {
{
if tokenizer . starts_with (b"^=") {
tokenizer . advance (2) ; PrefixMatch
} else {
tokenizer . advance (1) ; Delim ('^')
}
}
} , Case :: Case25 => {
{
tokenizer . advance (1) ; CurlyBracketBlock
}
} , Case :: Case26 => {
{
if tokenizer . starts_with (b"|=") {
tokenizer . advance (2) ; DashMatch
} else {
tokenizer . advance (1) ; Delim ('|')
}
}
} , Case :: Case27 => {
{
tokenizer . advance (1) ; CloseCurlyBracket
}
} , Case :: Case28 => {
{
if tokenizer . starts_with (b"~=") {
tokenizer . advance (2) ; IncludeMatch
} else {
tokenizer . advance (1) ; Delim ('~')
}
}
} , Case :: Case29 => {
{
if ! b . is_ascii () {
consume_ident_like (tokenizer)
} else {
tokenizer . advance (1) ; Delim (b as char)
}
}
}
}
} ; Ok (token)
} fn consume_whitespace < 'a > (tokenizer : & mut Tokenizer < 'a > , newline : bool) -> Token < 'a > {
let start_position = tokenizer . position () ; if newline {
tokenizer . consume_newline () ;
} else {
tokenizer . advance (1) ;
} while ! tokenizer . is_eof () {
let b = tokenizer . next_byte_unchecked () ; {
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize
} static __CASES : [Case ; 256] = [Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case2 , Case :: Case3 , Case :: Case2 , Case :: Case2 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3] ; match __CASES [b as usize] {
Case :: Case1 => {
{
tokenizer . advance (1) ;
}
} , Case :: Case2 => {
{
tokenizer . consume_newline () ;
}
} , Case :: Case3 => {
{
break
}
}
}
}
} WhiteSpace (tokenizer . slice_from (start_position))
} fn check_for_source_map < 'a > (tokenizer : & mut Tokenizer < 'a > , contents : & 'a str) {
let directive = "# sourceMappingURL=" ; let directive_old = "@ sourceMappingURL=" ; if contents . starts_with (directive) || contents . starts_with (directive_old) {
let contents = & contents [directive . len () ..] ; tokenizer . source_map_url = contents . split (| c | c == ' ' || c == '\t' || c == '\x0C' || c == '\r' || c == '\n') . next ()
} let directive = "# sourceURL=" ; let directive_old = "@ sourceURL=" ; if contents . starts_with (directive) || contents . starts_with (directive_old) {
let contents = & contents [directive . len () ..] ; tokenizer . source_url = contents . split (| c | c == ' ' || c == '\t' || c == '\x0C' || c == '\r' || c == '\n') . next ()
}
} fn consume_comment < 'a > (tokenizer : & mut Tokenizer < 'a >) -> & 'a str {
tokenizer . advance (2) ; let start_position = tokenizer . position () ; while ! tokenizer . is_eof () {
{
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize , Case5 = 5isize
} static __CASES : [Case ; 256] = [Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case2 , Case :: Case5 , Case :: Case2 , Case :: Case2 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case1 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4] ; match __CASES [tokenizer . next_byte_unchecked () as usize] {
Case :: Case1 => {
{
let end_position = tokenizer . position () ; tokenizer . advance (1) ; if tokenizer . next_byte () == Some (b'/') {
tokenizer . advance (1) ; let contents = tokenizer . slice (start_position .. end_position) ; check_for_source_map (tokenizer , contents) ; return contents
}
}
} , Case :: Case2 => {
{
tokenizer . consume_newline () ;
}
} , Case :: Case3 => {
{
tokenizer . consume_continuation_byte () ;
}
} , Case :: Case4 => {
{
tokenizer . consume_4byte_intro () ;
}
} , Case :: Case5 => {
{
tokenizer . advance (1) ;
}
}
}
}
} let contents = tokenizer . slice_from (start_position) ; check_for_source_map (tokenizer , contents) ; contents
} fn consume_string < 'a > (tokenizer : & mut Tokenizer < 'a > , single_quote : bool) -> Token < 'a > {
match consume_quoted_string (tokenizer , single_quote) {
Ok (value) => QuotedString (value) , Err (value) => BadString (value) ,
}
} # [doc = " Return `Err(())` on syntax error (ie. unescaped newline)"] fn consume_quoted_string < 'a > (tokenizer : & mut Tokenizer < 'a > , single_quote : bool ,) -> Result < CowRcStr < 'a > , CowRcStr < 'a > > {
tokenizer . advance (1) ; let start_pos = tokenizer . position () ; let mut string_bytes ; loop {
if tokenizer . is_eof () {
return Ok (tokenizer . slice_from (start_pos) . into ()) ;
} {
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize , Case5 = 5isize , Case6 = 6isize , Case7 = 7isize
} static __CASES : [Case ; 256] = [Case :: Case3 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case4 , Case :: Case7 , Case :: Case4 , Case :: Case4 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case1 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case2 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case3 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6] ; match __CASES [tokenizer . next_byte_unchecked () as usize] {
Case :: Case1 => {
{
if ! single_quote {
let value = tokenizer . slice_from (start_pos) ; tokenizer . advance (1) ; return Ok (value . into ())
} tokenizer . advance (1) ;
}
} , Case :: Case2 => {
{
if single_quote {
let value = tokenizer . slice_from (start_pos) ; tokenizer . advance (1) ; return Ok (value . into ())
} tokenizer . advance (1) ;
}
} , Case :: Case3 => {
{
string_bytes = tokenizer . slice_from (start_pos) . as_bytes () . to_owned () ; break
}
} , Case :: Case4 => {
{
return Err (tokenizer . slice_from (start_pos) . into ())
}
} , Case :: Case5 => {
{
tokenizer . consume_continuation_byte () ;
}
} , Case :: Case6 => {
{
tokenizer . consume_4byte_intro () ;
}
} , Case :: Case7 => {
{
tokenizer . advance (1) ;
}
}
}
}
} while ! tokenizer . is_eof () {
let b = tokenizer . next_byte_unchecked () ; {
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize , Case5 = 5isize , Case6 = 6isize , Case7 = 7isize , Case8 = 8isize
} static __CASES : [Case ; 256] = [Case :: Case5 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case1 , Case :: Case8 , Case :: Case1 , Case :: Case1 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case2 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case3 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case4 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7] ; match __CASES [b as usize] {
Case :: Case1 => {
{
return Err (unsafe {
from_utf8_release_unchecked (string_bytes)
} . into ()) ;
}
} , Case :: Case2 => {
{
tokenizer . advance (1) ; if ! single_quote {
break ;
}
}
} , Case :: Case3 => {
{
tokenizer . advance (1) ; if single_quote {
break ;
}
}
} , Case :: Case4 => {
{
tokenizer . advance (1) ; if ! tokenizer . is_eof () {
match tokenizer . next_byte_unchecked () {
b'\n' | b'\x0C' | b'\r' => {
tokenizer . consume_newline () ;
} _ => consume_escape_and_write (tokenizer , & mut string_bytes)
}
} continue ;
}
} , Case :: Case5 => {
{
tokenizer . advance (1) ; string_bytes . extend ("\u{FFFD}" . as_bytes ()) ; continue ;
}
} , Case :: Case6 => {
{
tokenizer . consume_continuation_byte () ;
}
} , Case :: Case7 => {
{
tokenizer . consume_4byte_intro () ;
}
} , Case :: Case8 => {
{
tokenizer . advance (1) ;
}
}
}
} string_bytes . push (b) ;
} Ok (unsafe {
from_utf8_release_unchecked (string_bytes)
} . into () ,)
} # [inline] fn is_ident_start (tokenizer : & mut Tokenizer) -> bool {
! tokenizer . is_eof () && {
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize
} static __CASES : [Case ; 256] = [Case :: Case1 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case2 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case4 , Case :: Case3 , Case :: Case4 , Case :: Case4 , Case :: Case1 , Case :: Case4 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4] ; let b = tokenizer . next_byte_unchecked () ; match __CASES [b as usize] {
Case :: Case1 => {
{
true
}
} , Case :: Case2 => {
{
tokenizer . has_at_least (1) && {
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize
} static __CASES : [Case ; 256] = [Case :: Case1 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case3 , Case :: Case2 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case3 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3] ; let b = tokenizer . byte_at (1) ; match __CASES [b as usize] {
Case :: Case1 => {
{
true
}
} , Case :: Case2 => {
{
! tokenizer . has_newline_at (1)
}
} , Case :: Case3 => {
{
! b . is_ascii ()
}
}
}
}
}
} , Case :: Case3 => {
{
! tokenizer . has_newline_at (1)
}
} , Case :: Case4 => {
{
! b . is_ascii ()
}
}
}
}
} fn consume_ident_like < 'a > (tokenizer : & mut Tokenizer < 'a >) -> Token < 'a > {
let value = consume_name (tokenizer) ; if ! tokenizer . is_eof () && tokenizer . next_byte_unchecked () == b'(' {
tokenizer . advance (1) ; if value . eq_ignore_ascii_case ("url") {
consume_unquoted_url (tokenizer) . unwrap_or (Function (value))
} else {
tokenizer . see_function (& value) ; Function (value)
}
} else {
Ident (value)
}
} fn consume_name < 'a > (tokenizer : & mut Tokenizer < 'a >) -> CowRcStr < 'a > {
let start_pos = tokenizer . position () ; let mut value_bytes ; loop {
if tokenizer . is_eof () {
return tokenizer . slice_from (start_pos) . into () ;
} {
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize , Case5 = 5isize , Case6 = 6isize
} static __CASES : [Case ; 256] = [Case :: Case2 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case1 , Case :: Case6 , Case :: Case6 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case6 , Case :: Case2 , Case :: Case6 , Case :: Case6 , Case :: Case1 , Case :: Case6 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5] ; let _b = tokenizer . next_byte_unchecked () ; match __CASES [_b as usize] {
Case :: Case1 => {
{
tokenizer . advance (1)
}
} , Case :: Case2 => {
{
value_bytes = tokenizer . slice_from (start_pos) . as_bytes () . to_owned () ; break
}
} , Case :: Case3 => {
{
tokenizer . consume_continuation_byte () ;
}
} , Case :: Case4 => {
{
tokenizer . advance (1) ;
}
} , Case :: Case5 => {
{
tokenizer . consume_4byte_intro () ;
}
} , Case :: Case6 => {
{
return tokenizer . slice_from (start_pos) . into () ;
}
}
}
}
} while ! tokenizer . is_eof () {
let b = tokenizer . next_byte_unchecked () ; {
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize , Case5 = 5isize , Case6 = 6isize , Case7 = 7isize
} static __CASES : [Case ; 256] = [Case :: Case3 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case1 , Case :: Case7 , Case :: Case7 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case7 , Case :: Case2 , Case :: Case7 , Case :: Case7 , Case :: Case1 , Case :: Case7 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6] ; match __CASES [b as usize] {
Case :: Case1 => {
{
tokenizer . advance (1) ; value_bytes . push (b)
}
} , Case :: Case2 => {
{
if tokenizer . has_newline_at (1) {
break
} tokenizer . advance (1) ; consume_escape_and_write (tokenizer , & mut value_bytes)
}
} , Case :: Case3 => {
{
tokenizer . advance (1) ; value_bytes . extend ("\u{FFFD}" . as_bytes ()) ;
}
} , Case :: Case4 => {
{
tokenizer . consume_continuation_byte () ; value_bytes . push (b)
}
} , Case :: Case5 => {
{
tokenizer . advance (1) ; value_bytes . push (b)
}
} , Case :: Case6 => {
{
tokenizer . consume_4byte_intro () ; value_bytes . push (b)
}
} , Case :: Case7 => {
{
break ;
}
}
}
}
} unsafe {
from_utf8_release_unchecked (value_bytes)
} . into ()
} fn byte_to_hex_digit (b : u8) -> Option < u32 > {
Some ({
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize
} static __CASES : [Case ; 256] = [Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case2 , Case :: Case2 , Case :: Case2 , Case :: Case2 , Case :: Case2 , Case :: Case2 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4] ; match __CASES [b as usize] {
Case :: Case1 => {
{
b - b'0'
}
} , Case :: Case2 => {
{
b - b'a' + 10
}
} , Case :: Case3 => {
{
b - b'A' + 10
}
} , Case :: Case4 => {
{
return None
}
}
}
} as u32)
} fn byte_to_decimal_digit (b : u8) -> Option < u32 > {
if b >= b'0' && b <= b'9' {
Some ((b - b'0') as u32)
} else {
None
}
} fn consume_numeric < 'a > (tokenizer : & mut Tokenizer < 'a >) -> Token < 'a > {
let (has_sign , sign) = match tokenizer . next_byte_unchecked () {
b'-' => (true , - 1.) , b'+' => (true , 1.) , _ => (false , 1.) ,
} ; if has_sign {
tokenizer . advance (1) ;
} let mut integral_part : f64 = 0. ; while let Some (digit) = byte_to_decimal_digit (tokenizer . next_byte_unchecked ()) {
integral_part = integral_part * 10. + digit as f64 ; tokenizer . advance (1) ; if tokenizer . is_eof () {
break ;
}
} let mut is_integer = true ; let mut fractional_part : f64 = 0. ; if tokenizer . has_at_least (1) && tokenizer . next_byte_unchecked () == b'.' && matches ! (tokenizer . byte_at (1) , b'0' ..= b'9') {
is_integer = false ; tokenizer . advance (1) ; let mut factor = 0.1 ; while let Some (digit) = byte_to_decimal_digit (tokenizer . next_byte_unchecked ()) {
fractional_part += digit as f64 * factor ; factor *= 0.1 ; tokenizer . advance (1) ; if tokenizer . is_eof () {
break ;
}
}
} let mut value = sign * (integral_part + fractional_part) ; if tokenizer . has_at_least (1) && matches ! (tokenizer . next_byte_unchecked () , b'e' | b'E') {
if matches ! (tokenizer . byte_at (1) , b'0' ..= b'9') || (tokenizer . has_at_least (2) && matches ! (tokenizer . byte_at (1) , b'+' | b'-') && matches ! (tokenizer . byte_at (2) , b'0' ..= b'9')) {
is_integer = false ; tokenizer . advance (1) ; let (has_sign , sign) = match tokenizer . next_byte_unchecked () {
b'-' => (true , - 1.) , b'+' => (true , 1.) , _ => (false , 1.) ,
} ; if has_sign {
tokenizer . advance (1) ;
} let mut exponent : f64 = 0. ; while let Some (digit) = byte_to_decimal_digit (tokenizer . next_byte_unchecked ()) {
exponent = exponent * 10. + digit as f64 ; tokenizer . advance (1) ; if tokenizer . is_eof () {
break ;
}
} value *= f64 :: powf (10. , sign * exponent) ;
}
} let int_value = if is_integer {
Some (if value >= i32 :: MAX as f64 {
i32 :: MAX
} else if value <= i32 :: MIN as f64 {
i32 :: MIN
} else {
value as i32
})
} else {
None
} ; if ! tokenizer . is_eof () && tokenizer . next_byte_unchecked () == b'%' {
tokenizer . advance (1) ; return Percentage {
unit_value : (value / 100.) as f32 , int_value : int_value , has_sign : has_sign ,
} ;
} let value = value as f32 ; if is_ident_start (tokenizer) {
let unit = consume_name (tokenizer) ; Dimension {
value : value , int_value : int_value , has_sign : has_sign , unit : unit ,
}
} else {
Number {
value : value , int_value : int_value , has_sign : has_sign ,
}
}
} # [inline] unsafe fn from_utf8_release_unchecked (string_bytes : Vec < u8 >) -> String {
if cfg ! (debug_assertions) {
String :: from_utf8 (string_bytes) . unwrap ()
} else {
String :: from_utf8_unchecked (string_bytes)
}
} fn consume_unquoted_url < 'a > (tokenizer : & mut Tokenizer < 'a >) -> Result < Token < 'a > , () > {
let start_position = tokenizer . position ; let from_start = & tokenizer . input [tokenizer . position ..] ; let mut newlines = 0 ; let mut last_newline = 0 ; let mut found_printable_char = false ; let mut iter = from_start . bytes () . enumerate () ; loop {
let (offset , b) = match iter . next () {
Some (item) => item , None => {
tokenizer . position = tokenizer . input . len () ; break ;
}
} ; {
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize , Case5 = 5isize , Case6 = 6isize
} static __CASES : [Case ; 256] = [Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case1 , Case :: Case2 , Case :: Case6 , Case :: Case2 , Case :: Case3 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case1 , Case :: Case6 , Case :: Case4 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case4 , Case :: Case6 , Case :: Case5 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6] ; match __CASES [b as usize] {
Case :: Case1 => {
{
}
} , Case :: Case2 => {
{
newlines += 1 ; last_newline = offset ;
}
} , Case :: Case3 => {
{
if from_start . as_bytes () . get (offset + 1) != Some (& b'\n') {
newlines += 1 ; last_newline = offset ;
}
}
} , Case :: Case4 => {
{
return Err (())
}
} , Case :: Case5 => {
{
tokenizer . position += offset + 1 ; break
}
} , Case :: Case6 => {
{
tokenizer . position += offset ; found_printable_char = true ; break
}
}
}
}
} if newlines > 0 {
tokenizer . current_line_number += newlines ; tokenizer . current_line_start_position = start_position + last_newline + 1 ;
} if found_printable_char {
return Ok (consume_unquoted_url_internal (tokenizer)) ;
} else {
return Ok (UnquotedUrl ("" . into ())) ;
} fn consume_unquoted_url_internal < 'a > (tokenizer : & mut Tokenizer < 'a >) -> Token < 'a > {
let start_pos = tokenizer . position () ; let mut string_bytes : Vec < u8 > ; loop {
if tokenizer . is_eof () {
return UnquotedUrl (tokenizer . slice_from (start_pos) . into ()) ;
} {
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize , Case5 = 5isize , Case6 = 6isize , Case7 = 7isize
} static __CASES : [Case ; 256] = [Case :: Case4 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case1 , Case :: Case3 , Case :: Case1 , Case :: Case1 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case7 , Case :: Case3 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case3 , Case :: Case3 , Case :: Case2 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case4 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case3 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case5 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6] ; match __CASES [tokenizer . next_byte_unchecked () as usize] {
Case :: Case1 => {
{
let value = tokenizer . slice_from (start_pos) ; return consume_url_end (tokenizer , start_pos , value . into ())
}
} , Case :: Case2 => {
{
let value = tokenizer . slice_from (start_pos) ; tokenizer . advance (1) ; return UnquotedUrl (value . into ())
}
} , Case :: Case3 => {
{
tokenizer . advance (1) ; return consume_bad_url (tokenizer , start_pos)
}
} , Case :: Case4 => {
{
string_bytes = tokenizer . slice_from (start_pos) . as_bytes () . to_owned () ; break
}
} , Case :: Case5 => {
{
tokenizer . consume_continuation_byte () ;
}
} , Case :: Case6 => {
{
tokenizer . consume_4byte_intro () ;
}
} , Case :: Case7 => {
{
tokenizer . advance (1) ;
}
}
}
}
} while ! tokenizer . is_eof () {
let b = tokenizer . next_byte_unchecked () ; {
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize , Case5 = 5isize , Case6 = 6isize , Case7 = 7isize , Case8 = 8isize
} static __CASES : [Case ; 256] = [Case :: Case5 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case1 , Case :: Case3 , Case :: Case1 , Case :: Case1 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case8 , Case :: Case3 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case3 , Case :: Case3 , Case :: Case2 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case4 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case3 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case6 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case8 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7 , Case :: Case7] ; let b = b ; match __CASES [b as usize] {
Case :: Case1 => {
{
let string = unsafe {
from_utf8_release_unchecked (string_bytes)
} . into () ; return consume_url_end (tokenizer , start_pos , string)
}
} , Case :: Case2 => {
{
tokenizer . advance (1) ; break ;
}
} , Case :: Case3 => {
{
tokenizer . advance (1) ; return consume_bad_url (tokenizer , start_pos) ;
}
} , Case :: Case4 => {
{
tokenizer . advance (1) ; if tokenizer . has_newline_at (0) {
return consume_bad_url (tokenizer , start_pos)
} consume_escape_and_write (tokenizer , & mut string_bytes)
}
} , Case :: Case5 => {
{
tokenizer . advance (1) ; string_bytes . extend ("\u{FFFD}" . as_bytes ()) ;
}
} , Case :: Case6 => {
{
tokenizer . consume_continuation_byte () ; string_bytes . push (b) ;
}
} , Case :: Case7 => {
{
tokenizer . consume_4byte_intro () ; string_bytes . push (b) ;
}
} , Case :: Case8 => {
{
tokenizer . advance (1) ; string_bytes . push (b)
}
}
}
}
} UnquotedUrl (unsafe {
from_utf8_release_unchecked (string_bytes)
} . into () ,)
} fn consume_url_end < 'a > (tokenizer : & mut Tokenizer < 'a > , start_pos : SourcePosition , string : CowRcStr < 'a > ,) -> Token < 'a > {
while ! tokenizer . is_eof () {
{
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize
} static __CASES : [Case ; 256] = [Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case2 , Case :: Case3 , Case :: Case4 , Case :: Case3 , Case :: Case3 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case2 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case1 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4] ; let b = tokenizer . next_byte_unchecked () ; match __CASES [b as usize] {
Case :: Case1 => {
{
tokenizer . advance (1) ; break
}
} , Case :: Case2 => {
{
tokenizer . advance (1) ;
}
} , Case :: Case3 => {
{
tokenizer . consume_newline () ;
}
} , Case :: Case4 => {
{
tokenizer . consume_known_byte (b) ; return consume_bad_url (tokenizer , start_pos) ;
}
}
}
}
} UnquotedUrl (string)
} fn consume_bad_url < 'a > (tokenizer : & mut Tokenizer < 'a > , start_pos : SourcePosition) -> Token < 'a > {
while ! tokenizer . is_eof () {
{
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize , Case4 = 4isize
} static __CASES : [Case ; 256] = [Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case3 , Case :: Case4 , Case :: Case3 , Case :: Case3 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case1 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case2 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4 , Case :: Case4] ; let b = tokenizer . next_byte_unchecked () ; match __CASES [b as usize] {
Case :: Case1 => {
{
let contents = tokenizer . slice_from (start_pos) . into () ; tokenizer . advance (1) ; return BadUrl (contents)
}
} , Case :: Case2 => {
{
tokenizer . advance (1) ; if matches ! (tokenizer . next_byte () , Some (b')') | Some (b'\\')) {
tokenizer . advance (1) ;
}
}
} , Case :: Case3 => {
{
tokenizer . consume_newline () ;
}
} , Case :: Case4 => {
{
tokenizer . consume_known_byte (b) ;
}
}
}
}
} BadUrl (tokenizer . slice_from (start_pos) . into ())
}
} fn consume_hex_digits < 'a > (tokenizer : & mut Tokenizer < 'a >) -> (u32 , u32) {
let mut value = 0 ; let mut digits = 0 ; while digits < 6 && ! tokenizer . is_eof () {
match byte_to_hex_digit (tokenizer . next_byte_unchecked ()) {
Some (digit) => {
value = value * 16 + digit ; digits += 1 ; tokenizer . advance (1) ;
} None => break ,
}
} (value , digits)
} fn consume_escape_and_write (tokenizer : & mut Tokenizer , bytes : & mut Vec < u8 >) {
bytes . extend (consume_escape (tokenizer) . encode_utf8 (& mut [0 ; 4]) . as_bytes () ,)
} fn consume_escape (tokenizer : & mut Tokenizer) -> char {
if tokenizer . is_eof () {
return '\u{FFFD}' ;
} {
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize
} static __CASES : [Case ; 256] = [Case :: Case2 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case1 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3] ; match __CASES [tokenizer . next_byte_unchecked () as usize] {
Case :: Case1 => {
{
let (c , _) = consume_hex_digits (tokenizer) ; if ! tokenizer . is_eof () {
{
enum Case {
Case1 = 1isize , Case2 = 2isize , Case3 = 3isize
} static __CASES : [Case ; 256] = [Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case2 , Case :: Case3 , Case :: Case2 , Case :: Case2 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case1 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3 , Case :: Case3] ; match __CASES [tokenizer . next_byte_unchecked () as usize] {
Case :: Case1 => {
{
tokenizer . advance (1)
}
} , Case :: Case2 => {
{
tokenizer . consume_newline () ;
}
} , Case :: Case3 => {
{
}
}
}
}
} static REPLACEMENT_CHAR : char = '\u{FFFD}' ; if c != 0 {
let c = char :: from_u32 (c) ; c . unwrap_or (REPLACEMENT_CHAR)
} else {
REPLACEMENT_CHAR
}
}
} , Case :: Case2 => {
{
tokenizer . advance (1) ; '\u{FFFD}'
}
} , Case :: Case3 => {
{
tokenizer . consume_char ()
}
}
}
}
}