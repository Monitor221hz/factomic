#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttributeId(u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TxnId(u64);

pub(crate) const SYSTEM_PARTITION: u8 = 0x01;
pub(crate) const USER_PARTITION: u8 = 0x02;
pub(crate) const TX_TIMESTAMP_ATTR: AttributeId = AttributeId(0x0001);
pub(crate) const FIRST_USER_ATTR: u16 = 0x0100;

impl EntityId {
    pub fn new(partition: u8, index: u64) -> Self {
        assert!(index < (1 << 56));
        let shifted = (partition as u64) << 56;
        EntityId(shifted | index)
    }

    pub fn with_value(value: u64) -> Self {
        EntityId(value)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn partition(&self) -> u8 {
        (self.0 >> 56) as u8
    }

    pub fn index(&self) -> u64 {
        self.0 & 0x00FF_FFFF_FFFF_FFFF
    }

    pub fn to_be_bytes(&self) -> [u8; 8] {
        self.0.to_be_bytes()
    }
}

impl AttributeId {
    pub fn new(id: u16) -> Self {
        AttributeId(id)
    }

    pub fn as_u16(&self) -> u16 {
        self.0
    }

    pub fn to_be_bytes(&self) -> [u8; size_of::<Self>()] {
        self.as_u16().to_be_bytes()
    }
}

impl TxnId {
    pub fn new(partition: u8, index: u64) -> Self {
        assert!(index < (1 << 56));
        let shifted = (partition as u64) << 56;
        TxnId(shifted | index)
    }

    pub fn with_value(id: u64) -> Self {
        TxnId(id)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn to_be_bytes(&self) -> [u8; size_of::<Self>()] {
        self.as_u64().to_be_bytes()
    }

    pub fn entity_id(&self) -> EntityId {
        EntityId::with_value(self.as_u64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_id_partition_round_trip() {
        let id = EntityId::new(USER_PARTITION, 42);
        assert_eq!(id.partition(), USER_PARTITION);
        assert_eq!(id.index(), 42);
    }

    #[test]
    #[should_panic]
    fn entity_id_index_overflow_panics() {
        let _ = EntityId::new(USER_PARTITION, 1 << 56);
    }

    #[test]
    fn entity_id_partition_sorts_before_index() {
        let system = EntityId::new(SYSTEM_PARTITION, u64::MAX >> 8);
        let user = EntityId::new(USER_PARTITION, 1);
        assert!(system.0.to_be_bytes() < user.0.to_be_bytes());
    }

    #[test]
    fn attribute_id_bytes_round_trip() {
        let id = AttributeId::new(0x0100);
        assert_eq!(u16::from_be_bytes(id.to_be_bytes()), id.as_u16());
    }

    #[test]
    fn txn_id_is_an_entity_id() {
        let txn = TxnId::new(SYSTEM_PARTITION, 7);
        assert_eq!(txn.entity_id().partition(), SYSTEM_PARTITION);
        assert_eq!(txn.entity_id().index(), 7);
    }
}
