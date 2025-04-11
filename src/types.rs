use ::rust_decimal::Decimal;
use std::str::FromStr;

#[derive(Debug, PartialEq, Eq)]
pub enum AccountType {
    Equity,
    Liability,
    Asset,
    Income,
    Expense,
}

/// Account identifier.
#[derive(Debug, PartialEq, Eq)]
pub struct AccountId {
    pub(crate) name: String,
    pub(crate) type_: AccountType,
}

#[derive(PartialEq, Eq, Debug)]
pub struct AccountFromStrError(String);

impl std::fmt::Display for AccountFromStrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unrecognized account type {}", self.0)
    }
}

impl std::error::Error for AccountFromStrError {}

impl FromStr for AccountType {
    type Err = AccountFromStrError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Equity" => Ok(AccountType::Equity),
            "Liabilities" => Ok(AccountType::Liability),
            "Assets" => Ok(AccountType::Asset),
            "Income" => Ok(AccountType::Income),
            "Expenses" => Ok(AccountType::Expense),
            x => Err(AccountFromStrError(x.to_string())),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Amount {
    pub(crate) currency: String,
    pub(crate) amount: Decimal,
}
