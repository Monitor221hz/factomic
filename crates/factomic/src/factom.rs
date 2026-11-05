use crate::error::{Error, Result};
use crate::id::{AttributeId, EntityId, TxnId};
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    Assert,
    Retract,
}

pub(crate) const ASSERT: u8 = 0x00;
pub(crate) const RETRACT: u8 = 0x01;

impl Op {
    pub fn to_byte(self) -> u8 {
        match self {
            Op::Assert => ASSERT,
            Op::Retract => RETRACT,
        }
    }

    pub fn from_byte(byte: u8) -> Result<Self> {
        match byte {
            ASSERT => Ok(Op::Assert),
            RETRACT => Ok(Op::Retract),
            _ => Err(Error::UnknownOp(byte)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Factom {
    entity: EntityId,
    attribute: AttributeId,
    value: Value,
    txn: TxnId,
    op: Op,
}

impl Factom {
    pub fn new(entity: EntityId, attribute: AttributeId, value: Value, txn: TxnId, op: Op) -> Self {
        Factom {
            entity,
            attribute,
            value,
            txn,
            op,
        }
    }

    pub fn entity(&self) -> EntityId {
        self.entity
    }

    pub fn attribute(&self) -> AttributeId {
        self.attribute
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn txn(&self) -> TxnId {
        self.txn
    }

    pub fn op(&self) -> Op {
        self.op
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{SYSTEM_PARTITION, USER_PARTITION};

    #[test]
    fn op_byte_round_trip() {
        assert_eq!(Op::from_byte(Op::Assert.to_byte()).unwrap(), Op::Assert);
        assert_eq!(Op::from_byte(Op::Retract.to_byte()).unwrap(), Op::Retract);
        assert!(matches!(Op::from_byte(0x02), Err(Error::UnknownOp(0x02))));
    }

    #[test]
    fn factom_accessors() {
        let e = EntityId::new(USER_PARTITION, 1);
        let a = AttributeId::new(0x0100);
        let t = TxnId::new(SYSTEM_PARTITION, 1);
        let factom = Factom::new(e, a, Value::Long(30), t, Op::Assert);
        assert_eq!(factom.entity(), e);
        assert_eq!(factom.attribute(), a);
        assert_eq!(factom.value(), &Value::Long(30));
        assert_eq!(factom.txn(), t);
        assert_eq!(factom.op(), Op::Assert);
    }
}
