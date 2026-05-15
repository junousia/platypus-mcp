def _release_file_impl(ctx):
    output = ctx.actions.declare_file(ctx.attr.out)
    ctx.actions.run_shell(
        inputs = [ctx.file.src],
        outputs = [output],
        command = "cp \"$1\" \"$2\" && chmod 755 \"$2\"",
        arguments = [
            ctx.file.src.path,
            output.path,
        ],
        mnemonic = "ReleaseFile",
        progress_message = "Staging %{label}",
    )
    return [DefaultInfo(files = depset([output]))]

release_file = rule(
    implementation = _release_file_impl,
    attrs = {
        "src": attr.label(allow_single_file = True, mandatory = True),
        "out": attr.string(mandatory = True),
    },
)

def _stamped_manifest_impl(ctx):
    output = ctx.actions.declare_file(ctx.attr.out)
    ctx.actions.run(
        inputs = [
            ctx.file.template,
            ctx.version_file,
        ],
        outputs = [output],
        executable = ctx.executable._tool,
        arguments = [
            "--input",
            ctx.file.template.path,
            "--output",
            output.path,
            "--package",
            ctx.attr.package_name,
            "--workspace-status",
            ctx.version_file.path,
        ],
        mnemonic = "StampedManifest",
        progress_message = "Stamping %{label}",
    )
    return [DefaultInfo(files = depset([output]))]

stamped_manifest = rule(
    implementation = _stamped_manifest_impl,
    attrs = {
        "out": attr.string(mandatory = True),
        "package_name": attr.string(mandatory = True),
        "template": attr.label(allow_single_file = True, mandatory = True),
        "_tool": attr.label(
            cfg = "exec",
            default = "//tools/release:stamp_manifest",
            executable = True,
        ),
    },
)
