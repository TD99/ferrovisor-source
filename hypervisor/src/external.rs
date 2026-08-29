pub struct ExternalUefiImage {
    pub name: &'static str,
    pub efi: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/external_guests.rs"));
