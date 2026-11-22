use crate::Factom;
use crate::Op;
use crate::Result;
use crate::TxnId;
use crate::keys;
impl Factom {
    pub(crate) fn with_easvt_pair(key: &[u8], payload: &[u8]) -> Result<Factom> {
        let (e, a, s, v) = keys::decode_easvt_key(key)?;
        let txn = TxnId::try_from(payload)?;
        Ok(Factom::new(e, a, s, v, txn, Op::Assert))
    }
    pub(crate) fn with_easvt_h_pair(key: &[u8], payload: &[u8]) -> Result<Factom> {
        let (e, a, k, v, txn) = keys::decode_easvt_history_key(key)?;
        let op = Op::try_from(payload[0])?;
        Ok(Factom::new(e, a, k, v, txn, op))
    }
    pub(crate) fn with_txlog_pair(key: &[u8], payload: &[u8]) -> Result<Factom> {
        let (txn, entity, attribute, slot, value) = keys::decode_txlog_key(key)?;
        let op = Op::try_from(payload[0])?;
        Ok(Factom::new(entity, attribute, slot, value, txn, op))
    }
}
