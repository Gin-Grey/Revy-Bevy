use bevy::{picking::pointer::PointerButton, prelude::*};

use crate::viewport::{PrimaryPointerOwner, PrimaryPointerOwnership, ViewportNavigationState};

/// Currently selected scene object (if any).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Selection(pub Option<Entity>);

/// Ordered multi-selection whose last entry is the primary Inspector target.
/// `Selection` stays as the compatibility-facing primary selection.
#[derive(Resource, Default, Debug, Clone)]
pub struct SelectionSet {
    primary: Option<Entity>,
    entities: Vec<Entity>,
}

impl SelectionSet {
    pub fn entities<'a>(&'a self, selection: &'a Selection) -> impl Iterator<Item = Entity> + 'a {
        let coherent = self.primary == selection.0;
        self.entities
            .iter()
            .copied()
            .filter(move |_| coherent)
            .chain(selection.0.into_iter().filter(move |_| !coherent))
    }

    pub fn contains(&self, selection: &Selection, entity: Entity) -> bool {
        self.entities(selection).any(|selected| selected == entity)
    }

    pub fn select_only(&mut self, selection: &mut Selection, entity: Entity) {
        self.entities.clear();
        self.entities.push(entity);
        self.primary = Some(entity);
        selection.0 = Some(entity);
    }

    pub fn select_many(
        &mut self,
        selection: &mut Selection,
        entities: impl IntoIterator<Item = Entity>,
    ) {
        self.entities.clear();
        for entity in entities {
            self.add(entity);
        }
        self.primary = self.entities.last().copied();
        selection.0 = self.primary;
    }

    pub fn select_from_click(
        &mut self,
        selection: &mut Selection,
        entity: Entity,
        shift: bool,
        control: bool,
    ) {
        self.sync_primary(selection);
        if control {
            self.toggle(entity);
        } else if shift {
            self.add(entity);
        } else {
            self.entities.clear();
            self.entities.push(entity);
        }
        self.primary = self.entities.last().copied();
        selection.0 = self.primary;
    }

    pub fn select_from_box(
        &mut self,
        selection: &mut Selection,
        entities: impl IntoIterator<Item = Entity>,
        shift: bool,
        control: bool,
    ) {
        self.sync_primary(selection);
        let hits: Vec<_> = entities.into_iter().collect();
        if control {
            for entity in hits {
                self.toggle(entity);
            }
        } else if shift {
            for entity in hits {
                self.add(entity);
            }
        } else {
            self.entities.clear();
            for entity in hits {
                self.add(entity);
            }
        }
        self.primary = self.entities.last().copied();
        selection.0 = self.primary;
    }

    fn sync_primary(&mut self, selection: &Selection) {
        if self.primary != selection.0 {
            self.entities.clear();
            self.entities.extend(selection.0);
            self.primary = selection.0;
        }
    }

    fn add(&mut self, entity: Entity) {
        if let Some(index) = self
            .entities
            .iter()
            .position(|selected| *selected == entity)
        {
            self.entities.remove(index);
        }
        self.entities.push(entity);
    }

    fn toggle(&mut self, entity: Entity) {
        if let Some(index) = self
            .entities
            .iter()
            .position(|selected| *selected == entity)
        {
            self.entities.remove(index);
        } else {
            self.entities.push(entity);
        }
    }
}

/// Marks objects that appear in the Hierarchy and can be selected.
#[derive(Component, Debug, Clone)]
pub struct EditableObject {
    pub name: String,
}

pub fn on_mesh_click(
    click: On<Pointer<Click>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    navigation: Res<ViewportNavigationState>,
    ownership: Res<PrimaryPointerOwnership>,
    mut selection: ResMut<Selection>,
    mut selection_set: ResMut<SelectionSet>,
    editable: Query<&EditableObject>,
) {
    let camera_modifier = keyboard.pressed(KeyCode::Space)
        || keyboard.pressed(KeyCode::AltLeft)
        || keyboard.pressed(KeyCode::AltRight);
    if click.button != PointerButton::Primary
        || camera_modifier
        || navigation.blocks_primary_selection()
        || ownership.selection_is_locked_by(PrimaryPointerOwner::Sprite)
        || (ownership.is_claimed() && !ownership.is_owned_by(PrimaryPointerOwner::Sprite))
    {
        return;
    }

    // Ignore clicks that are not on editable scene objects (UI, ground, etc.).
    if editable.get(click.entity).is_err() {
        return;
    }

    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let control = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    selection_set.select_from_click(&mut selection, click.entity, shift, control);
}

pub fn select_entity(entity: Entity, selection: &mut Selection) {
    selection.0 = Some(entity);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn click_selection_supports_replace_add_and_toggle() {
        let a = Entity::from_bits(1);
        let b = Entity::from_bits(2);
        let mut selection = Selection::default();
        let mut set = SelectionSet::default();

        set.select_from_click(&mut selection, a, false, false);
        set.select_from_click(&mut selection, b, true, false);
        assert_eq!(set.entities(&selection).collect::<Vec<_>>(), vec![a, b]);
        assert_eq!(selection.0, Some(b));

        set.select_from_click(&mut selection, a, false, true);
        assert_eq!(set.entities(&selection).collect::<Vec<_>>(), vec![b]);
        assert_eq!(selection.0, Some(b));
    }

    #[test]
    fn legacy_primary_assignment_falls_back_to_single_selection() {
        let a = Entity::from_bits(1);
        let b = Entity::from_bits(2);
        let mut selection = Selection::default();
        let mut set = SelectionSet::default();
        set.select_from_click(&mut selection, a, false, false);

        selection.0 = Some(b);

        assert_eq!(set.entities(&selection).collect::<Vec<_>>(), vec![b]);
        assert!(set.contains(&selection, b));
        assert_eq!(set.entities(&selection).count(), 1);
    }

    #[test]
    fn box_selection_replaces_adds_and_toggles_as_one_operation() {
        let a = Entity::from_bits(1);
        let b = Entity::from_bits(2);
        let c = Entity::from_bits(3);
        let mut selection = Selection::default();
        let mut set = SelectionSet::default();

        set.select_from_box(&mut selection, [a, b], false, false);
        set.select_from_box(&mut selection, [c], true, false);
        assert_eq!(set.entities(&selection).collect::<Vec<_>>(), vec![a, b, c]);

        set.select_from_box(&mut selection, [a, b], false, true);
        assert_eq!(set.entities(&selection).collect::<Vec<_>>(), vec![c]);
        assert_eq!(selection.0, Some(c));
    }

    #[test]
    fn replacing_selection_preserves_order_and_uses_the_last_entity_as_primary() {
        let a = Entity::from_bits(1);
        let b = Entity::from_bits(2);
        let mut selection = Selection::default();
        let mut set = SelectionSet::default();

        set.select_many(&mut selection, [a, b, a]);

        assert_eq!(set.entities(&selection).collect::<Vec<_>>(), vec![b, a]);
        assert_eq!(selection.0, Some(a));
    }
}
