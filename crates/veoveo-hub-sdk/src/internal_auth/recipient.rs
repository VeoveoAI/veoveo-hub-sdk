//! The existing recipient comparison, separated for manual shared-source proofs.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recipient<S, D, R> {
    pub server: S,
    pub deployment: D,
    pub resource: R,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mismatch {
    Audience,
    Deployment,
}

pub fn bind<S: PartialEq, D: PartialEq, R: PartialEq>(
    expected: Recipient<S, D, R>,
    presented: Recipient<S, D, R>,
) -> Result<Recipient<S, D, R>, Mismatch> {
    if expected.server != presented.server || expected.resource != presented.resource {
        return Err(Mismatch::Audience);
    }
    if expected.deployment != presented.deployment {
        return Err(Mismatch::Deployment);
    }
    Ok(presented)
}
