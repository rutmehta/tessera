//! WB diagnostic call-site hooks (rev7 4.1). The default arms expand to the
//! original expressions; the feature arms route through
//! `wb_diagnostic::live`; with no token they touch no arena state.

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
    ($r:expr, $batch:expr, $key:expr, $b:ident) => {
        crate::wb_diagnostic::live::lookup($r.diag, $batch, &$key, crate::wb_diagnostic::Bucket::$b)
    };
}
#[cfg(feature = "wb-diagnostic")]
macro_rules! wb_request {
    ($r:expr, $key:expr, $b:ident) => {
        if let Some(t) = $r.diag {
            crate::wb_diagnostic::live::request(t, &$key, crate::wb_diagnostic::Bucket::$b)
        }
    };
}
