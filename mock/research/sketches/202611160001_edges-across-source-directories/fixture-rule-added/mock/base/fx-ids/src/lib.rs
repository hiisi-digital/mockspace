pub struct Id(pub u32);

pub const fn tile_of(id: &Id) -> u32 {
    id.0 / fx_units::TILE
}
