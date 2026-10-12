//! Protect and unlock through Ghostscript.

use super::util::{os, run_gs};
use super::Ctx;
use crate::{Error, Permissions, ProtectOptions, Result};

/// Standard-security-handler /P value (revision 3) as a signed 32-bit integer.
pub(crate) fn permission_bits(p: &Permissions) -> i32 {
    let mut bits: u32 = 0xFFFF_F0C0;
    if p.allow_print {
        bits |= 4 | 2048;
    }
    if p.allow_modify {
        bits |= 8 | 1024;
    }
    if p.allow_copy {
        bits |= 16 | 512;
    }
    if p.allow_annotate {
        bits |= 32 | 256;
    }
    bits as i32
}

pub(super) fn protect(ctx: &mut Ctx<'_>, o: &ProtectOptions) -> Result<()> {
    if o.user_password.is_empty() && o.owner_password.is_empty() {
        return Err(Error::InvalidOptions("Enter a password".into()));
    }
    let owner = if o.owner_password.is_empty() { &o.user_password } else { &o.owner_password };
    let tmp = tempfile::tempdir()?;
    let out = tmp.path().join("protected.pdf");
    ctx.report(0.1, "Encrypting");
    let extra = vec![
        os("-dCompatibilityLevel=1.6"),
        os(format!("-sOwnerPassword={owner}")),
        os(format!("-sUserPassword={}", o.user_password)),
        os("-dEncryptionR=3"),
        os("-dKeyLength=128"),
        os(format!("-dPermissions={}", permission_bits(&o.permissions))),
    ];
    let input = ctx.input_path(0).to_path_buf();
    run_gs(&extra, &input, &out, ctx.password(0), ctx.cancel)?;
    let stem = ctx.req.output_stem.clone();
    ctx.emit_file(&stem, "pdf", &out)?;
    Ok(())
}

pub(super) fn unlock(ctx: &mut Ctx<'_>) -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let out = tmp.path().join("unlocked.pdf");
    ctx.report(0.1, "Removing protection");
    let input = ctx.input_path(0).to_path_buf();
    run_gs(&[os("-dCompatibilityLevel=1.7")], &input, &out, ctx.password(0), ctx.cancel)?;
    let stem = ctx.req.output_stem.clone();
    ctx.emit_file(&stem, "pdf", &out)?;
    Ok(())
}
