use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrimaryPointerOwner {
    Navigation,
    Ui,
    Sprite,
    Marquee,
}

/// Owns one complete primary-button press, from press through release.
#[derive(Resource, Debug, Default)]
pub(crate) struct PrimaryPointerOwnership {
    owner: Option<PrimaryPointerOwner>,
    selection_locked: bool,
}

impl PrimaryPointerOwnership {
    pub(crate) fn claim(&mut self, owner: PrimaryPointerOwner) -> bool {
        match self.owner {
            None => {
                self.owner = Some(owner);
                true
            }
            Some(current) => current == owner,
        }
    }

    pub(crate) fn is_owned_by(&self, owner: PrimaryPointerOwner) -> bool {
        self.owner == Some(owner)
    }

    pub(crate) fn is_claimed(&self) -> bool {
        self.owner.is_some()
    }

    pub(crate) fn lock_selection(&mut self, owner: PrimaryPointerOwner) -> bool {
        if self.owner != Some(owner) {
            return false;
        }
        self.selection_locked = true;
        true
    }

    pub(crate) fn selection_is_locked_by(&self, owner: PrimaryPointerOwner) -> bool {
        self.owner == Some(owner) && self.selection_locked
    }

    fn release(&mut self) {
        self.owner = None;
        self.selection_locked = false;
    }
}

pub(super) fn release_primary_pointer_ownership(
    mouse: Res<ButtonInput<MouseButton>>,
    mut ownership: ResMut<PrimaryPointerOwnership>,
) {
    if !mouse.pressed(MouseButton::Left) {
        ownership.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_primary_press_has_exactly_one_owner() {
        let mut ownership = PrimaryPointerOwnership::default();

        assert!(ownership.claim(PrimaryPointerOwner::Ui));
        assert!(ownership.claim(PrimaryPointerOwner::Ui));
        assert!(!ownership.claim(PrimaryPointerOwner::Sprite));
        assert!(!ownership.claim(PrimaryPointerOwner::Marquee));
        assert!(ownership.is_owned_by(PrimaryPointerOwner::Ui));
    }

    #[test]
    fn transform_edit_can_lock_selection_until_release() {
        let mut ownership = PrimaryPointerOwnership::default();
        assert!(ownership.claim(PrimaryPointerOwner::Sprite));
        assert!(ownership.lock_selection(PrimaryPointerOwner::Sprite));
        assert!(ownership.selection_is_locked_by(PrimaryPointerOwner::Sprite));

        ownership.release();

        assert!(!ownership.selection_is_locked_by(PrimaryPointerOwner::Sprite));
    }

    #[test]
    fn release_allows_the_next_press_to_choose_a_new_owner() {
        let mut ownership = PrimaryPointerOwnership::default();
        assert!(ownership.claim(PrimaryPointerOwner::Navigation));

        ownership.release();

        assert!(ownership.claim(PrimaryPointerOwner::Sprite));
        assert!(ownership.is_owned_by(PrimaryPointerOwner::Sprite));
    }
}
