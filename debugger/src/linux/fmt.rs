use std::fmt;

impl fmt::Debug for super::Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPathName => f.write_str("There appears to be a '\\0' in the path name."),
            Self::IncompleteRead(req, res) => {
                f.write_fmt(format_args!("Tried to read {req} bytes, only read {res}."))
            }
            Self::IncompleteWrite(req, res) => f.write_fmt(format_args!(
                "Tried to write {req} bytes, only wrote {res}."
            )),
            Self::Kernel(err) => f.write_fmt(format_args!("{err}.")),
        }
    }
}
