mod check;
mod release;
mod setup;
mod test;
mod version;

use argh::FromArgs;

use crate::Result;

#[derive(FromArgs)]
#[argh(subcommand)]
pub enum Task {
    Test(test::Test),
    Check(check::Check),
    Setup(setup::Setup),
    // Version(Version),
    // Release(Release),
}

impl Task {
    pub fn run(self) -> Result {
        match self {
            Task::Test(cmd) => cmd.run(),
            Task::Check(cmd) => cmd.run(),
            Task::Setup(cmd) => cmd.run(),
        }
    }
}