//! WB diagnostic call-site hooks (rev7 4.1). The default arms expand to the
//! original expressions; the feature arms route through
//! `wb_diagnostic::live`. B0: the feature arms ignore the token.

#[cfg(not(feature = "wb-diagnostic"))]
macro_rules! wb_lookup {
    ($r:expr, $batch:expr, $key:expr, $b:ident) => {
        $batch.cached(&$key)
    };
}
#[cfg(not(feature = "wb-diagnostic"))]
macro_rules! wb_request {
    ($r:expr, $key:expr, $b:ident) => {};
}
#[cfg(feature = "wb-diagnostic")]
macro_rules! wb_lookup {
    ($r:expr, $batch:expr, $key:expr, $b:ident) => {{
        let _ = &$r.diag;
        $batch.cached(&$key)
    }};
}
#[cfg(feature = "wb-diagnostic")]
macro_rules! wb_request {
    ($r:expr, $key:expr, $b:ident) => {{
        let _ = &$r.diag;
    }};
}
