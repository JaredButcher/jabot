# JABot
JABot
A rust discord bot with features such as orchestrating a secret santa

https://github.com/serenity-rs/serenity

# Setup
In order to compile the project, a database needs to be set-up. That's because SQLx accesses the database at compile time to make sure your SQL queries are correct.

https://github.com/launchbadge/sqlx/tree/main/sqlx-cli

To set up the database, download the SQLx CLI and run `sqlx database setup`. This command will create the database and its tables by applying the migration files in `migrations/`.

Most SQLx CLI commands require the `DATABASE_URL` environment variable to be set to the database URL, for example `sqlite:database.sqlite` (where `sqlite:` is the protocol and `database.sqlite` the actual filename). A convenient way to supply this information to SQLx is to create a `.env` file which SQLx automatically detects and reads:

```DATABASE_URL=sqlite:database.sqlite```

Expects an envirment variable `DISCORD_TOKEN_FILE` with a parth to a file containing a discord api token.

Build with `cargo build`.

# Run
Can be run with without enviroment files. Though `.env` should be used.

```DATABASE_URL=sqlite:examples/e16_sqlite_database/database.sqlite DISCORD_TOKEN_FILE=... cargo run```
