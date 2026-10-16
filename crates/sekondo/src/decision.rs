//! What the SSO callback does with a verified character. Pure, so every branch is unit-tested:
//!
//! ```text
//!                      +-- Login ----------+-- char unknown         -> new account, sign in
//! callback -> verify --+                   +-- char known, owner ok -> resume account, sign in
//!                      |                   +-- owner hash changed   -> delete char, then "unknown"
//!                      +-- AddCharacter ---+-- char unknown         -> link to session account
//!                                          +-- char in same account -> refresh tokens
//!                                          +-- char in other account (same owner) -> reject
//!                                          +-- owner hash changed   -> delete char, then link
//! ```

use uuid::Uuid;

use crate::oauth::Intent;

/// A character that is already linked to some account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExistingCharacter {
    pub account_id: Uuid,
    /// The owner hash SSO reported equals the stored one.
    pub owner_hash_matches: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// The character belongs to a different account and must be unlinked there first.
    BelongsToOtherAccount,
    /// "Add character" finished but nobody is signed in any more.
    NoSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Sign in to a new account holding only this character. `remove_existing`: the character
    /// changed hands, so its old record (and data) is deleted first.
    CreateAccount {
        remove_existing: bool,
    },
    /// Sign in to the account the character already belongs to; tokens are replaced.
    ResumeAccount {
        account_id: Uuid,
    },
    /// Link the character to the signed-in account, removing a transferred record first.
    LinkToSessionAccount {
        account_id: Uuid,
        remove_existing: bool,
    },
    /// The character is already in the signed-in account: only its tokens are replaced.
    RefreshInSessionAccount {
        account_id: Uuid,
    },
    Reject(RejectReason),
}

pub fn decide(
    intent: Intent,
    session_account: Option<Uuid>,
    existing: Option<ExistingCharacter>,
) -> Decision {
    match intent {
        Intent::Login => match existing {
            None => Decision::CreateAccount {
                remove_existing: false,
            },
            Some(c) if c.owner_hash_matches => Decision::ResumeAccount {
                account_id: c.account_id,
            },
            Some(_) => Decision::CreateAccount {
                remove_existing: true,
            },
        },
        Intent::AddCharacter => {
            let Some(session) = session_account else {
                return Decision::Reject(RejectReason::NoSession);
            };
            match existing {
                None => Decision::LinkToSessionAccount {
                    account_id: session,
                    remove_existing: false,
                },
                Some(c) if !c.owner_hash_matches => Decision::LinkToSessionAccount {
                    account_id: session,
                    remove_existing: true,
                },
                Some(c) if c.account_id == session => Decision::RefreshInSessionAccount {
                    account_id: session,
                },
                Some(_) => Decision::Reject(RejectReason::BelongsToOtherAccount),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing(account_id: Uuid, owner_hash_matches: bool) -> Option<ExistingCharacter> {
        Some(ExistingCharacter {
            account_id,
            owner_hash_matches,
        })
    }

    #[test]
    fn login_with_an_unknown_character_creates_an_account() {
        assert_eq!(
            decide(Intent::Login, None, None),
            Decision::CreateAccount {
                remove_existing: false
            }
        );
    }

    #[test]
    fn login_with_a_known_character_resumes_its_account() {
        let a = Uuid::new_v4();
        assert_eq!(
            decide(Intent::Login, None, existing(a, true)),
            Decision::ResumeAccount { account_id: a }
        );
    }

    #[test]
    fn login_with_a_transferred_character_removes_it_and_creates_an_account() {
        assert_eq!(
            decide(Intent::Login, None, existing(Uuid::new_v4(), false)),
            Decision::CreateAccount {
                remove_existing: true
            }
        );
    }

    #[test]
    fn login_ignores_a_leftover_session_account() {
        let (session, owner) = (Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(
            decide(Intent::Login, Some(session), existing(owner, true)),
            Decision::ResumeAccount { account_id: owner }
        );
    }

    #[test]
    fn add_with_an_unknown_character_links_it_to_the_session_account() {
        let s = Uuid::new_v4();
        assert_eq!(
            decide(Intent::AddCharacter, Some(s), None),
            Decision::LinkToSessionAccount {
                account_id: s,
                remove_existing: false
            }
        );
    }

    #[test]
    fn add_with_a_character_of_the_same_account_refreshes_it() {
        let s = Uuid::new_v4();
        assert_eq!(
            decide(Intent::AddCharacter, Some(s), existing(s, true)),
            Decision::RefreshInSessionAccount { account_id: s }
        );
    }

    #[test]
    fn add_with_a_character_of_another_account_is_rejected() {
        assert_eq!(
            decide(
                Intent::AddCharacter,
                Some(Uuid::new_v4()),
                existing(Uuid::new_v4(), true)
            ),
            Decision::Reject(RejectReason::BelongsToOtherAccount)
        );
    }

    #[test]
    fn add_with_a_transferred_character_removes_it_and_links_it() {
        let s = Uuid::new_v4();
        for old_account in [s, Uuid::new_v4()] {
            assert_eq!(
                decide(Intent::AddCharacter, Some(s), existing(old_account, false)),
                Decision::LinkToSessionAccount {
                    account_id: s,
                    remove_existing: true
                },
                "old record in {old_account}"
            );
        }
    }

    #[test]
    fn add_without_a_session_is_rejected() {
        for e in [
            None,
            existing(Uuid::new_v4(), true),
            existing(Uuid::new_v4(), false),
        ] {
            assert_eq!(
                decide(Intent::AddCharacter, None, e),
                Decision::Reject(RejectReason::NoSession)
            );
        }
    }
}
