#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub demo: bool,
    pub open: bool,
    pub verbose: bool,
    pub demo_battery: Option<u8>,
    pub demo_name: Option<String>,
    pub uninstall_cleanup: bool,
    pub quiet: bool,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Args {
    let mut out = Args::default();
    let mut it = args.into_iter().peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--demo" => out.demo = true,
            "--open" => out.open = true,
            "--verbose" => out.verbose = true,
            "--uninstall-cleanup" => out.uninstall_cleanup = true,
            "--quiet" => out.quiet = true,
            "--demo-battery" => {
                // Only consume the next token when it is a valid percentage.
                if let Some(n) = it
                    .peek()
                    .and_then(|v| v.parse::<u8>().ok())
                    .filter(|n| *n <= 100)
                {
                    out.demo_battery = Some(n);
                    it.next();
                }
            }
            "--demo-name" => out.demo_name = it.next_if(|v| !v.starts_with("--")),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Args {
        parse(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn args_defaults() {
        assert_eq!(p(&[]), Args::default());
    }

    #[test]
    fn args_all_flags() {
        let a = p(&[
            "--demo",
            "--open",
            "--verbose",
            "--demo-battery",
            "19",
            "--uninstall-cleanup",
            "--quiet",
        ]);
        assert!(a.demo && a.open && a.verbose && a.uninstall_cleanup && a.quiet);
        assert_eq!(a.demo_battery, Some(19));
    }

    #[test]
    fn demo_name() {
        let a = p(&["--demo-name", "My very long headphones name", "--demo"]);
        assert_eq!(a.demo_name.as_deref(), Some("My very long headphones name"));
        assert!(a.demo);
        assert_eq!(p(&["--demo-name", "--demo"]).demo_name, None);
        assert_eq!(p(&["--demo-name"]).demo_name, None);
    }

    #[test]
    fn demo_battery_bad_value_ignored() {
        assert_eq!(p(&["--demo-battery", "abc"]).demo_battery, None);
        assert_eq!(p(&["--demo-battery", "300"]).demo_battery, None);
        assert_eq!(p(&["--demo-battery"]).demo_battery, None);
        assert!(p(&["--bogus", "--demo"]).demo);
    }
}
