
/// Unroll the given for loop
///
/// Example:
///
/// ```ignore
/// unroll! {
///   for i in 0..5 {
///     println!("Iteration {}", i);
///   }
/// }
/// ```
///
/// will expand into:
///
/// ```ignore
/// { println!("Iteration {}", 0); }
/// { println!("Iteration {}", 1); }
/// { println!("Iteration {}", 2); }
/// { println!("Iteration {}", 3); }
/// { println!("Iteration {}", 4); }
/// ```
#[macro_export]
macro_rules! unroll {
    (for $v:ident in 0..0 $c:block) => {};

    (for $v:ident < $max:tt in ($start:tt..$end:tt).step_by($val:expr) {$($c:tt)*}) => {
        {
            let step = $val;
            let start = $start;
            let end = start + ($end - start) / step;
            unroll! {
                for val < $max in start..end {
                    let $v: usize = ((val - start) * step) + start;

                    $($c)*
                }
            }
        }
    };

    (for $v:ident in ($start:tt..$end:tt).step_by($val:expr) {$($c:tt)*}) => {
        unroll! {
            for $v < $end in ($start..$end).step_by($val) {$($c)*}
        }
    };

    (for $v:ident in ($start:tt..$end:tt) {$($c:tt)*}) => {
        unroll!{
            for $v in $start..$end {$($c)*}
        }
    };

    (for $v:ident in $start:tt..$end:tt {$($c:tt)*}) => {
        #[allow(non_upper_case_globals)]
        #[allow(unused_comparisons)]
        {
            unroll!(@$v, 0, $end, {
                    if $v >= $start {$($c)*}
                }
            );
        }
    };

    (for $v:ident < $max:tt in $start:tt..$end:tt $c:block) => {
        #[allow(non_upper_case_globals)]
        {
            let range = $start..$end;
            assert!(
                $max >= range.end,
                "`{}` out of range `{:?}`",
                stringify!($max),
                range,
            );
            unroll!(
                @$v,
                0,
                $max,
                {
                    if $v >= range.start && $v < range.end {
                        $c
                    }
                }
            );
        }
    };

    (for $v:ident in 0..$end:tt {$($statement:tt)*}) => {
        #[allow(non_upper_case_globals)]
        { unroll!(@$v, 0, $end, {$($statement)*}); }
    };

    (@$v:ident, $a:expr, 0, $c:block) => {
        { const $v: usize = $a; $c }
    };

    (@$v:ident, $a:expr, 1, $c:block) => {
        { const $v: usize = $a; $c }
    };

    (@$v:ident, $a:expr, 2, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
    };

    (@$v:ident, $a:expr, 3, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
    };

    (@$v:ident, $a:expr, 4, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
    };

    (@$v:ident, $a:expr, 5, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
        { const $v: usize = $a + 4; $c }
    };

    (@$v:ident, $a:expr, 6, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
        { const $v: usize = $a + 4; $c }
        { const $v: usize = $a + 5; $c }
    };

    (@$v:ident, $a:expr, 7, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
        { const $v: usize = $a + 4; $c }
        { const $v: usize = $a + 5; $c }
        { const $v: usize = $a + 6; $c }
    };

    (@$v:ident, $a:expr, 8, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
        { const $v: usize = $a + 4; $c }
        { const $v: usize = $a + 5; $c }
        { const $v: usize = $a + 6; $c }
        { const $v: usize = $a + 7; $c }
    };

    (@$v:ident, $a:expr, 9, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
        { const $v: usize = $a + 4; $c }
        { const $v: usize = $a + 5; $c }
        { const $v: usize = $a + 6; $c }
        { const $v: usize = $a + 7; $c }
        { const $v: usize = $a + 8; $c }
    };

    (@$v:ident, $a:expr, 10, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
        { const $v: usize = $a + 4; $c }
        { const $v: usize = $a + 5; $c }
        { const $v: usize = $a + 6; $c }
        { const $v: usize = $a + 7; $c }
        { const $v: usize = $a + 8; $c }
        { const $v: usize = $a + 9; $c }
    };

    (@$v:ident, $a:expr, 11, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
        { const $v: usize = $a + 4; $c }
        { const $v: usize = $a + 5; $c }
        { const $v: usize = $a + 6; $c }
        { const $v: usize = $a + 7; $c }
        { const $v: usize = $a + 8; $c }
        { const $v: usize = $a + 9; $c }
        { const $v: usize = $a + 10; $c }
    };

    (@$v:ident, $a:expr, 12, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
        { const $v: usize = $a + 4; $c }
        { const $v: usize = $a + 5; $c }
        { const $v: usize = $a + 6; $c }
        { const $v: usize = $a + 7; $c }
        { const $v: usize = $a + 8; $c }
        { const $v: usize = $a + 9; $c }
        { const $v: usize = $a + 10; $c }
        { const $v: usize = $a + 11; $c }
    };

    (@$v:ident, $a:expr, 13, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
        { const $v: usize = $a + 4; $c }
        { const $v: usize = $a + 5; $c }
        { const $v: usize = $a + 6; $c }
        { const $v: usize = $a + 7; $c }
        { const $v: usize = $a + 8; $c }
        { const $v: usize = $a + 9; $c }
        { const $v: usize = $a + 10; $c }
        { const $v: usize = $a + 11; $c }
        { const $v: usize = $a + 12; $c }
    };

    (@$v:ident, $a:expr, 14, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
        { const $v: usize = $a + 4; $c }
        { const $v: usize = $a + 5; $c }
        { const $v: usize = $a + 6; $c }
        { const $v: usize = $a + 7; $c }
        { const $v: usize = $a + 8; $c }
        { const $v: usize = $a + 9; $c }
        { const $v: usize = $a + 10; $c }
        { const $v: usize = $a + 11; $c }
        { const $v: usize = $a + 12; $c }
        { const $v: usize = $a + 13; $c }
    };

    (@$v:ident, $a:expr, 15, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
        { const $v: usize = $a + 4; $c }
        { const $v: usize = $a + 5; $c }
        { const $v: usize = $a + 6; $c }
        { const $v: usize = $a + 7; $c }
        { const $v: usize = $a + 8; $c }
        { const $v: usize = $a + 9; $c }
        { const $v: usize = $a + 10; $c }
        { const $v: usize = $a + 11; $c }
        { const $v: usize = $a + 12; $c }
        { const $v: usize = $a + 13; $c }
        { const $v: usize = $a + 14; $c }
    };

    (@$v:ident, $a:expr, 16, $c:block) => {
        { const $v: usize = $a; $c }
        { const $v: usize = $a + 1; $c }
        { const $v: usize = $a + 2; $c }
        { const $v: usize = $a + 3; $c }
        { const $v: usize = $a + 4; $c }
        { const $v: usize = $a + 5; $c }
        { const $v: usize = $a + 6; $c }
        { const $v: usize = $a + 7; $c }
        { const $v: usize = $a + 8; $c }
        { const $v: usize = $a + 9; $c }
        { const $v: usize = $a + 10; $c }
        { const $v: usize = $a + 11; $c }
        { const $v: usize = $a + 12; $c }
        { const $v: usize = $a + 13; $c }
        { const $v: usize = $a + 14; $c }
        { const $v: usize = $a + 15; $c }
    };

    (@$v:ident, $a:expr, 17, $c:block) => {
        unroll!(@$v, $a, 16, $c);
        { const $v: usize = $a + 16; $c }
    };

    (@$v:ident, $a:expr, 18, $c:block) => {
        unroll!(@$v, $a, 9, $c);
        unroll!(@$v, $a + 9, 9, $c);
    };

    (@$v:ident, $a:expr, 19, $c:block) => {
        unroll!(@$v, $a, 18, $c);
        { const $v: usize = $a + 18; $c }
    };

    (@$v:ident, $a:expr, 20, $c:block) => {
        unroll!(@$v, $a, 10, $c);
        unroll!(@$v, $a + 10, 10, $c);
    };

    (@$v:ident, $a:expr, 21, $c:block) => {
        unroll!(@$v, $a, 20, $c);
        { const $v: usize = $a + 20; $c }
    };

    (@$v:ident, $a:expr, 22, $c:block) => {
        unroll!(@$v, $a, 11, $c);
        unroll!(@$v, $a + 11, 11, $c);
    };

    (@$v:ident, $a:expr, 23, $c:block) => {
        unroll!(@$v, $a, 22, $c);
        { const $v: usize = $a + 22; $c }
    };

    (@$v:ident, $a:expr, 24, $c:block) => {
        unroll!(@$v, $a, 12, $c);
        unroll!(@$v, $a + 12, 12, $c);
    };

    (@$v:ident, $a:expr, 25, $c:block) => {
        unroll!(@$v, $a, 24, $c);
        { const $v: usize = $a + 24; $c }
    };

    (@$v:ident, $a:expr, 26, $c:block) => {
        unroll!(@$v, $a, 13, $c);
        unroll!(@$v, $a + 13, 13, $c);
    };

    (@$v:ident, $a:expr, 27, $c:block) => {
        unroll!(@$v, $a, 26, $c);
        { const $v: usize = $a + 26; $c }
    };

    (@$v:ident, $a:expr, 28, $c:block) => {
        unroll!(@$v, $a, 14, $c);
        unroll!(@$v, $a + 14, 14, $c);
    };

    (@$v:ident, $a:expr, 29, $c:block) => {
        unroll!(@$v, $a, 28, $c);
        { const $v: usize = $a + 28; $c }
    };

    (@$v:ident, $a:expr, 30, $c:block) => {
        unroll!(@$v, $a, 15, $c);
        unroll!(@$v, $a + 15, 15, $c);
    };

    (@$v:ident, $a:expr, 31, $c:block) => {
        unroll!(@$v, $a, 30, $c);
        { const $v: usize = $a + 30; $c }
    };

    (@$v:ident, $a:expr, 32, $c:block) => {
        unroll!(@$v, $a, 16, $c);
        unroll!(@$v, $a + 16, 16, $c);
    };

    (@$v:ident, $a:expr, 33, $c:block) => {
        unroll!(@$v, $a, 32, $c);
        { const $v: usize = $a + 32; $c }
    };

    (@$v:ident, $a:expr, 34, $c:block) => {
        unroll!(@$v, $a, 17, $c);
        unroll!(@$v, $a + 17, 17, $c);
    };

    (@$v:ident, $a:expr, 35, $c:block) => {
        unroll!(@$v, $a, 34, $c);
        { const $v: usize = $a + 34; $c }
    };

    (@$v:ident, $a:expr, 36, $c:block) => {
        unroll!(@$v, $a, 18, $c);
        unroll!(@$v, $a + 18, 18, $c);
    };

    (@$v:ident, $a:expr, 37, $c:block) => {
        unroll!(@$v, $a, 36, $c);
        { const $v: usize = $a + 36; $c }
    };

    (@$v:ident, $a:expr, 38, $c:block) => {
        unroll!(@$v, $a, 19, $c);
        unroll!(@$v, $a + 19, 19, $c);
    };

    (@$v:ident, $a:expr, 39, $c:block) => {
        unroll!(@$v, $a, 38, $c);
        { const $v: usize = $a + 38; $c }
    };

    (@$v:ident, $a:expr, 40, $c:block) => {
        unroll!(@$v, $a, 20, $c);
        unroll!(@$v, $a + 20, 20, $c);
    };

    (@$v:ident, $a:expr, 41, $c:block) => {
        unroll!(@$v, $a, 40, $c);
        { const $v: usize = $a + 40; $c }
    };

    (@$v:ident, $a:expr, 42, $c:block) => {
        unroll!(@$v, $a, 21, $c);
        unroll!(@$v, $a + 21, 21, $c);
    };

    (@$v:ident, $a:expr, 43, $c:block) => {
        unroll!(@$v, $a, 42, $c);
        { const $v: usize = $a + 42; $c }
    };

    (@$v:ident, $a:expr, 44, $c:block) => {
        unroll!(@$v, $a, 22, $c);
        unroll!(@$v, $a + 22, 22, $c);
    };

    (@$v:ident, $a:expr, 45, $c:block) => {
        unroll!(@$v, $a, 44, $c);
        { const $v: usize = $a + 44; $c }
    };

    (@$v:ident, $a:expr, 46, $c:block) => {
        unroll!(@$v, $a, 23, $c);
        unroll!(@$v, $a + 23, 23, $c);
    };

    (@$v:ident, $a:expr, 47, $c:block) => {
        unroll!(@$v, $a, 46, $c);
        { const $v: usize = $a + 46; $c }
    };

    (@$v:ident, $a:expr, 48, $c:block) => {
        unroll!(@$v, $a, 24, $c);
        unroll!(@$v, $a + 24, 24, $c);
    };

    (@$v:ident, $a:expr, 49, $c:block) => {
        unroll!(@$v, $a, 48, $c);
        { const $v: usize = $a + 48; $c }
    };

    (@$v:ident, $a:expr, 50, $c:block) => {
        unroll!(@$v, $a, 25, $c);
        unroll!(@$v, $a + 25, 25, $c);
    };

    (@$v:ident, $a:expr, 51, $c:block) => {
        unroll!(@$v, $a, 50, $c);
        { const $v: usize = $a + 50; $c }
    };

    (@$v:ident, $a:expr, 52, $c:block) => {
        unroll!(@$v, $a, 26, $c);
        unroll!(@$v, $a + 26, 26, $c);
    };

    (@$v:ident, $a:expr, 53, $c:block) => {
        unroll!(@$v, $a, 52, $c);
        { const $v: usize = $a + 52; $c }
    };

    (@$v:ident, $a:expr, 54, $c:block) => {
        unroll!(@$v, $a, 27, $c);
        unroll!(@$v, $a + 27, 27, $c);
    };

    (@$v:ident, $a:expr, 55, $c:block) => {
        unroll!(@$v, $a, 54, $c);
        { const $v: usize = $a + 54; $c }
    };

    (@$v:ident, $a:expr, 56, $c:block) => {
        unroll!(@$v, $a, 28, $c);
        unroll!(@$v, $a + 28, 28, $c);
    };

    (@$v:ident, $a:expr, 57, $c:block) => {
        unroll!(@$v, $a, 56, $c);
        { const $v: usize = $a + 56; $c }
    };

    (@$v:ident, $a:expr, 58, $c:block) => {
        unroll!(@$v, $a, 29, $c);
        unroll!(@$v, $a + 29, 29, $c);
    };

    (@$v:ident, $a:expr, 59, $c:block) => {
        unroll!(@$v, $a, 58, $c);
        { const $v: usize = $a + 58; $c }
    };

    (@$v:ident, $a:expr, 60, $c:block) => {
        unroll!(@$v, $a, 30, $c);
        unroll!(@$v, $a + 30, 30, $c);
    };

    (@$v:ident, $a:expr, 61, $c:block) => {
        unroll!(@$v, $a, 60, $c);
        { const $v: usize = $a + 60; $c }
    };

    (@$v:ident, $a:expr, 62, $c:block) => {
        unroll!(@$v, $a, 31, $c);
        unroll!(@$v, $a + 31, 31, $c);
    };

    (@$v:ident, $a:expr, 63, $c:block) => {
        unroll!(@$v, $a, 62, $c);
        { const $v: usize = $a + 62; $c }
    };

    (@$v:ident, $a:expr, 64, $c:block) => {
        unroll!(@$v, $a, 32, $c);
        unroll!(@$v, $a + 32, 32, $c);
    };

    (@$v:ident, $a:expr, 65, $c:block) => {
        unroll!(@$v, $a, 64, $c);
        { const $v: usize = $a + 64; $c }
    };

    (@$v:ident, $a:expr, 66, $c:block) => {
        unroll!(@$v, $a, 33, $c);
        unroll!(@$v, $a + 33, 33, $c);
    };

    (@$v:ident, $a:expr, 67, $c:block) => {
        unroll!(@$v, $a, 66, $c);
        { const $v: usize = $a + 66; $c }
    };

    (@$v:ident, $a:expr, 68, $c:block) => {
        unroll!(@$v, $a, 34, $c);
        unroll!(@$v, $a + 34, 34, $c);
    };

    (@$v:ident, $a:expr, 69, $c:block) => {
        unroll!(@$v, $a, 68, $c);
        { const $v: usize = $a + 68; $c }
    };

    (@$v:ident, $a:expr, 70, $c:block) => {
        unroll!(@$v, $a, 35, $c);
        unroll!(@$v, $a + 35, 35, $c);
    };

    (@$v:ident, $a:expr, 71, $c:block) => {
        unroll!(@$v, $a, 70, $c);
        { const $v: usize = $a + 70; $c }
    };

    (@$v:ident, $a:expr, 72, $c:block) => {
        unroll!(@$v, $a, 36, $c);
        unroll!(@$v, $a + 36, 36, $c);
    };

    (@$v:ident, $a:expr, 73, $c:block) => {
        unroll!(@$v, $a, 72, $c);
        { const $v: usize = $a + 72; $c }
    };

    (@$v:ident, $a:expr, 74, $c:block) => {
        unroll!(@$v, $a, 37, $c);
        unroll!(@$v, $a + 37, 37, $c);
    };

    (@$v:ident, $a:expr, 75, $c:block) => {
        unroll!(@$v, $a, 74, $c);
        { const $v: usize = $a + 74; $c }
    };

    (@$v:ident, $a:expr, 76, $c:block) => {
        unroll!(@$v, $a, 38, $c);
        unroll!(@$v, $a + 38, 38, $c);
    };

    (@$v:ident, $a:expr, 77, $c:block) => {
        unroll!(@$v, $a, 76, $c);
        { const $v: usize = $a + 76; $c }
    };

    (@$v:ident, $a:expr, 78, $c:block) => {
        unroll!(@$v, $a, 39, $c);
        unroll!(@$v, $a + 39, 39, $c);
    };

    (@$v:ident, $a:expr, 79, $c:block) => {
        unroll!(@$v, $a, 78, $c);
        { const $v: usize = $a + 78; $c }
    };

    (@$v:ident, $a:expr, 80, $c:block) => {
        unroll!(@$v, $a, 40, $c);
        unroll!(@$v, $a + 40, 40, $c);
    };

    (@$v:ident, $a:expr, 81, $c:block) => {
        unroll!(@$v, $a, 80, $c);
        { const $v: usize = $a + 80; $c }
    };

    (@$v:ident, $a:expr, 82, $c:block) => {
        unroll!(@$v, $a, 41, $c);
        unroll!(@$v, $a + 41, 41, $c);
    };

    (@$v:ident, $a:expr, 83, $c:block) => {
        unroll!(@$v, $a, 82, $c);
        { const $v: usize = $a + 82; $c }
    };

    (@$v:ident, $a:expr, 84, $c:block) => {
        unroll!(@$v, $a, 42, $c);
        unroll!(@$v, $a + 42, 42, $c);
    };

    (@$v:ident, $a:expr, 85, $c:block) => {
        unroll!(@$v, $a, 84, $c);
        { const $v: usize = $a + 84; $c }
    };

    (@$v:ident, $a:expr, 86, $c:block) => {
        unroll!(@$v, $a, 43, $c);
        unroll!(@$v, $a + 43, 43, $c);
    };

    (@$v:ident, $a:expr, 87, $c:block) => {
        unroll!(@$v, $a, 86, $c);
        { const $v: usize = $a + 86; $c }
    };

    (@$v:ident, $a:expr, 88, $c:block) => {
        unroll!(@$v, $a, 44, $c);
        unroll!(@$v, $a + 44, 44, $c);
    };

    (@$v:ident, $a:expr, 89, $c:block) => {
        unroll!(@$v, $a, 88, $c);
        { const $v: usize = $a + 88; $c }
    };

    (@$v:ident, $a:expr, 90, $c:block) => {
        unroll!(@$v, $a, 45, $c);
        unroll!(@$v, $a + 45, 45, $c);
    };

    (@$v:ident, $a:expr, 91, $c:block) => {
        unroll!(@$v, $a, 90, $c);
        { const $v: usize = $a + 90; $c }
    };

    (@$v:ident, $a:expr, 92, $c:block) => {
        unroll!(@$v, $a, 46, $c);
        unroll!(@$v, $a + 46, 46, $c);
    };

    (@$v:ident, $a:expr, 93, $c:block) => {
        unroll!(@$v, $a, 92, $c);
        { const $v: usize = $a + 92; $c }
    };

    (@$v:ident, $a:expr, 94, $c:block) => {
        unroll!(@$v, $a, 47, $c);
        unroll!(@$v, $a + 47, 47, $c);
    };

    (@$v:ident, $a:expr, 95, $c:block) => {
        unroll!(@$v, $a, 94, $c);
        { const $v: usize = $a + 94; $c }
    };

    (@$v:ident, $a:expr, 96, $c:block) => {
        unroll!(@$v, $a, 48, $c);
        unroll!(@$v, $a + 48, 48, $c);
    };

    (@$v:ident, $a:expr, 97, $c:block) => {
        unroll!(@$v, $a, 96, $c);
        { const $v: usize = $a + 96; $c }
    };

    (@$v:ident, $a:expr, 98, $c:block) => {
        unroll!(@$v, $a, 49, $c);
        unroll!(@$v, $a + 49, 49, $c);
    };

    (@$v:ident, $a:expr, 99, $c:block) => {
        unroll!(@$v, $a, 98, $c);
        { const $v: usize = $a + 98; $c }
    };

    (@$v:ident, $a:expr, 100, $c:block) => {
        unroll!(@$v, $a, 50, $c);
        unroll!(@$v, $a + 50, 50, $c);
    };

    (@$v:ident, $a:expr, 101, $c:block) => {
        unroll!(@$v, $a, 100, $c);
        { const $v: usize = $a + 100; $c }
    };

    (@$v:ident, $a:expr, 102, $c:block) => {
        unroll!(@$v, $a, 51, $c);
        unroll!(@$v, $a + 51, 51, $c);
    };

    (@$v:ident, $a:expr, 103, $c:block) => {
        unroll!(@$v, $a, 102, $c);
        { const $v: usize = $a + 102; $c }
    };

    (@$v:ident, $a:expr, 104, $c:block) => {
        unroll!(@$v, $a, 52, $c);
        unroll!(@$v, $a + 52, 52, $c);
    };

    (@$v:ident, $a:expr, 105, $c:block) => {
        unroll!(@$v, $a, 104, $c);
        { const $v: usize = $a + 104; $c }
    };

    (@$v:ident, $a:expr, 106, $c:block) => {
        unroll!(@$v, $a, 53, $c);
        unroll!(@$v, $a + 53, 53, $c);
    };

    (@$v:ident, $a:expr, 107, $c:block) => {
        unroll!(@$v, $a, 106, $c);
        { const $v: usize = $a + 106; $c }
    };

    (@$v:ident, $a:expr, 108, $c:block) => {
        unroll!(@$v, $a, 54, $c);
        unroll!(@$v, $a + 54, 54, $c);
    };

    (@$v:ident, $a:expr, 109, $c:block) => {
        unroll!(@$v, $a, 108, $c);
        { const $v: usize = $a + 108; $c }
    };

    (@$v:ident, $a:expr, 110, $c:block) => {
        unroll!(@$v, $a, 55, $c);
        unroll!(@$v, $a + 55, 55, $c);
    };

    (@$v:ident, $a:expr, 111, $c:block) => {
        unroll!(@$v, $a, 110, $c);
        { const $v: usize = $a + 110; $c }
    };

    (@$v:ident, $a:expr, 112, $c:block) => {
        unroll!(@$v, $a, 56, $c);
        unroll!(@$v, $a + 56, 56, $c);
    };

    (@$v:ident, $a:expr, 113, $c:block) => {
        unroll!(@$v, $a, 112, $c);
        { const $v: usize = $a + 112; $c }
    };

    (@$v:ident, $a:expr, 114, $c:block) => {
        unroll!(@$v, $a, 57, $c);
        unroll!(@$v, $a + 57, 57, $c);
    };

    (@$v:ident, $a:expr, 115, $c:block) => {
        unroll!(@$v, $a, 114, $c);
        { const $v: usize = $a + 114; $c }
    };

    (@$v:ident, $a:expr, 116, $c:block) => {
        unroll!(@$v, $a, 58, $c);
        unroll!(@$v, $a + 58, 58, $c);
    };

    (@$v:ident, $a:expr, 117, $c:block) => {
        unroll!(@$v, $a, 116, $c);
        { const $v: usize = $a + 116; $c }
    };

    (@$v:ident, $a:expr, 118, $c:block) => {
        unroll!(@$v, $a, 59, $c);
        unroll!(@$v, $a + 59, 59, $c);
    };

    (@$v:ident, $a:expr, 119, $c:block) => {
        unroll!(@$v, $a, 118, $c);
        { const $v: usize = $a + 118; $c }
    };

    (@$v:ident, $a:expr, 120, $c:block) => {
        unroll!(@$v, $a, 60, $c);
        unroll!(@$v, $a + 60, 60, $c);
    };

    (@$v:ident, $a:expr, 121, $c:block) => {
        unroll!(@$v, $a, 120, $c);
        { const $v: usize = $a + 120; $c }
    };

    (@$v:ident, $a:expr, 122, $c:block) => {
        unroll!(@$v, $a, 61, $c);
        unroll!(@$v, $a + 61, 61, $c);
    };

    (@$v:ident, $a:expr, 123, $c:block) => {
        unroll!(@$v, $a, 122, $c);
        { const $v: usize = $a + 122; $c }
    };

    (@$v:ident, $a:expr, 124, $c:block) => {
        unroll!(@$v, $a, 62, $c);
        unroll!(@$v, $a + 62, 62, $c);
    };

    (@$v:ident, $a:expr, 125, $c:block) => {
        unroll!(@$v, $a, 124, $c);
        { const $v: usize = $a + 124; $c }
    };

    (@$v:ident, $a:expr, 126, $c:block) => {
        unroll!(@$v, $a, 63, $c);
        unroll!(@$v, $a + 63, 63, $c);
    };

    (@$v:ident, $a:expr, 127, $c:block) => {
        unroll!(@$v, $a, 126, $c);
        { const $v: usize = $a + 126; $c }
    };

    (@$v:ident, $a:expr, 128, $c:block) => {
        unroll!(@$v, $a, 64, $c);
        unroll!(@$v, $a + 64, 64, $c);
    };

}


#[cfg(all(test, feature = "std"))]
mod tests {
    #[test]
    fn invalid_range() {
        let mut a: Vec<usize> = vec![];
        unroll! {
                for i in (5..4) {
                    a.push(i);
                }
            }
        assert_eq!(a, vec![]);
    }

    #[test]
    fn start_at_one_with_step() {
        let mut a: Vec<usize> = vec![];
        unroll! {
                for i in (2..4).step_by(1) {
                    a.push(i);
                }
            }
        assert_eq!(a, vec![2, 3]);
    }

    #[test]
    fn start_at_one() {
        let mut a: Vec<usize> = vec![];
        unroll! {
                for i in 1..4 {
                    a.push(i);
                }
            }
        assert_eq!(a, vec![1, 2, 3]);
    }

    #[test]
    fn test_all() {
        {
            let a: Vec<usize> = vec![];
            unroll! {
                for i in 0..0 {
                    a.push(i);
                }
            }
            assert_eq!(a, (0..0).collect::<Vec<usize>>());
        }
        {
            let mut a: Vec<usize> = vec![];
            unroll! {
                for i in 0..1 {
                    a.push(i);
                }
            }
            assert_eq!(a, (0..1).collect::<Vec<usize>>());
        }
        {
            let mut a: Vec<usize> = vec![];
            unroll! {
                for i in 0..128 {
                    a.push(i);
                }
            }
            assert_eq!(a, (0..128).collect::<Vec<usize>>());
        }
        {
            let mut a: Vec<usize> = vec![];
            let start = 128 / 4;
            let end = start * 3;
            unroll! {
                for i < 128 in start..end {
                    a.push(i);
                }
            }
            assert_eq!(a, (start..end).collect::<Vec<usize>>());
        }
        {
            let mut a: Vec<usize> = vec![];
            unroll! {
                for i in (0..128).step_by(2) {
                    a.push(i);
                }
            }
            assert_eq!(a, (0..128 / 2).map(|x| x * 2).collect::<Vec<usize>>());
        }
        {
            let mut a: Vec<usize> = vec![];
            let start = 128 / 4;
            let end = start * 3;
            unroll! {
                for i < 128 in (start..end).step_by(2) {
                    a.push(i);
                }
            }
            assert_eq!(a, (start..end).filter(|x| x % 2 == 0).collect::<Vec<usize>>());
        }
    }
}
