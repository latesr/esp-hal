use crate::soc::regi2c::{regi2c_read, regi2c_write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) struct RegI2cMaster {
    pub master: u8,
    pub host_id: u8,
}

impl RegI2cMaster {
    pub const fn new(master: u8, host_id: u8) -> Self {
        Self { master, host_id }
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn write(&self, reg: u8, data: u8) {
        regi2c_write(self.master, self.host_id, reg, data);
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn read(&self, reg: u8) -> u8 {
        regi2c_read(self.master, self.host_id, reg)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) struct RegI2cRegister {
    master: RegI2cMaster,
    reg_add: u8,
}

impl RegI2cRegister {
    pub const fn new(master: RegI2cMaster, reg_add: u8) -> Self {
        Self { master, reg_add }
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn write_reg(&self, data: u8) {
        self.master.write(self.reg_add, data);
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn read(&self) -> u8 {
        self.master.read(self.reg_add)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) struct RawRegI2cField {
    register: RegI2cRegister,
    msb: u8,
    lsb: u8,
}

impl RawRegI2cField {
    #[allow(unused)]
    pub const fn new(register: RegI2cRegister, msb: u8, lsb: u8) -> Self {
        ::core::assert!(lsb <= msb && msb < 8);
        Self { register, msb, lsb }
    }

    #[inline(always)]
    const fn mask(&self) -> u8 {
        const fn lower_n_bits(bits: u8) -> u8 {
            !(u32::MAX.wrapping_shl(bits as u32) as u8)
        }

        lower_n_bits(self.msb.wrapping_sub(self.lsb).wrapping_add(1))
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn read(&self) -> u8 {
        self.register.read().wrapping_shr(u32::from(self.lsb)) & self.mask()
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn write_field(&self, data: u8) {
        self.write_field_with(data, regi2c_read, regi2c_write);
    }

    /// Writes the field through `read` and `write`, which take the master, its host ID and the
    /// register address, like `regi2c_read` and `regi2c_write`.
    ///
    /// Code that runs with the caches suspended passes accessors in RAM, and this function must not
    /// panic.
    #[allow(unused)]
    #[inline(always)]
    pub fn write_field_with(
        &self,
        data: u8,
        read: impl Fn(u8, u8, u8) -> u8,
        write: impl Fn(u8, u8, u8, u8),
    ) {
        let RegI2cRegister { master, reg_add } = self.register;
        let lsb = u32::from(self.lsb);
        let mask = self.mask().wrapping_shl(lsb);
        let bits = read(master.master, master.host_id, reg_add);
        write(
            master.master,
            master.host_id,
            reg_add,
            (bits & !mask) | (data.wrapping_shl(lsb) & mask),
        );
    }
}

macro_rules! define_regi2c {
    ($(master: $master_name:ident($master:literal, $hostid:literal) {
        $(reg: $register_name:ident($register:literal) {
            $(field: $field_name:ident ( $msb:literal .. $lsb:literal )),*
        })*
    })+) => {
        $(
            #[allow(unused)]
            pub(crate) const $master_name: RegI2cMaster = RegI2cMaster::new($master, $hostid);

            $(
                #[allow(unused)]
                pub(crate) const $register_name: RegI2cRegister = RegI2cRegister::new($master_name, $register);

                $(
                    #[allow(unused)]
                    pub(crate) const $field_name: RawRegI2cField = RawRegI2cField::new($register_name, $msb, $lsb);
                )*
            )*
        )+
    };
}
pub(crate) use define_regi2c;
