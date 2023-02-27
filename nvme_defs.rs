




pub const NVME_QUEUE_DEPTH: usize = 1024;


#[derive(Default, Clone, Copy, Debug)]
#[repr(transparent)]
#[allow(non_camel_case_types)]
pub struct le<T>(T);

impl<T> le<T> {
    pub fn into(self) -> T {
        self.0
    }
}

impl<T> From<T> for le<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

#[allow(non_camel_case_types)]
pub enum NvmeAdminOpcode {
    delete_sq = 0x00,
    create_sq = 0x01,
    get_log_page = 0x02,
    delete_cq = 0x04,
    create_cq = 0x05,
    identify = 0x06,
    abort_cmd = 0x08,
    set_features = 0x09,
    get_features = 0x0a,
    async_event = 0x0c,
    activate_fw = 0x10,
    download_fw = 0x11,
    dbbuf = 0x7c,
    format_nvm = 0x80,
    security_send = 0x81,
    security_recv = 0x82,
}

#[allow(non_camel_case_types)]
#[derive(Debug)]
pub enum NvmeOpcode {
    flush = 0x00,
    write = 0x01,
    read = 0x02,
    write_uncor = 0x04,
    compare = 0x05,
    dsm = 0x09,
}

pub const NVME_QUEUE_PHYS_CONTIG: u16 = 1 << 0;
pub const NVME_CQ_IRQ_ENABLED: u16 = 1 << 1;
pub const NVME_SQ_PRIO_URGENT: u16 = 0 << 1;
pub const NVME_SQ_PRIO_HIGH: u16 = 1 << 1;
pub const NVME_SQ_PRIO_MEDIUM: u16 = 2 << 1;
pub const NVME_SQ_PRIO_LOW: u16 = 3 << 1;

pub const NVME_FEAT_ARBITRATION: u32 = 0x01;
pub const NVME_FEAT_POWER_MGMT: u32 = 0x02;
pub const NVME_FEAT_LBA_RANGE: u32 = 0x03;
pub const NVME_FEAT_TEMP_THRESH: u32 = 0x04;
pub const NVME_FEAT_ERR_RECOVERY: u32 = 0x05;
pub const NVME_FEAT_VOLATILE_WC: u32 = 0x06;
pub const NVME_FEAT_NUM_QUEUES: u32 = 0x07;
pub const NVME_FEAT_IRQ_COALESCE: u32 = 0x08;
pub const NVME_FEAT_IRQ_CONFIG: u32 = 0x09;
pub const NVME_FEAT_WRITE_ATOMIC: u32 = 0x0a;
pub const NVME_FEAT_ASYNC_EVENT: u32 = 0x0b;
pub const NVME_FEAT_SW_PROGRESS: u32 = 0x0c;

#[derive(Default, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeCompletion {
    pub result: le<u32>,
    reserved: u32,
    pub sq_head: le<u16>,
    pub sq_id: le<u16>,
    pub command_id: u16,
    pub status: le<u16>,
}

#[derive(Default, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeCreateCq {
    pub opcode: u8,
    pub flags: u8,
    pub command_id: u16,
    pub rsvd1: [u32; 5],
    pub prp1: le<u64>,
    pub rsvd8: u64,
    pub cqid: le<u16>,
    pub qsize: le<u16>,
    pub cq_flags: le<u16>,
    pub irq_vector: le<u16>,
    pub rsvd12: [u32; 4],
}

#[derive(Default, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeCreateSq {
    pub opcode: u8,
    pub flags: u8,
    pub command_id: u16,
    pub rsvd1: [u32; 5],
    pub prp1: le<u64>,
    pub rsvd8: u64,
    pub sqid: le<u16>,
    pub qsize: le<u16>,
    pub sq_flags: le<u16>,
    pub cqid: le<u16>,
    pub rsvd12: [u32; 4],
}

#[derive(Default, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeIdentify {
    pub opcode: u8,
    pub flags: u8,
    pub command_id: u16,
    pub nsid: le<u32>,
    pub reserved1: [u64; 2],
    pub prp1: le<u64>,
    pub prp2: le<u64>,
    pub cns: le<u32>,
    pub reserved2: [u32; 5],
}

#[derive(Default, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeFeatures {
    pub opcode: u8,
    pub flags: u8,
    pub command_id: u16,
    pub nsid: le<u32>,
    pub rsvd2: [u64; 2],
    pub prp1: le<u64>,
    pub prp2: le<u64>,
    pub fid: le<u32>,
    pub dword11: le<u32>,
    pub rsvd12: [u32; 4],
}

#[derive(Default, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeLbaRangeType {
    pub type_: u8,
    pub attributes: u8,
    rsvd2: [u8; 14],
    pub slba: le<u64>,
    pub nlb: le<u64>,
    pub guid: [u8; 16],
    rsvd48: [u8; 16],
}
pub const NVME_LBART_ATTRIB_TEMP: u8 = 1 << 0;
pub const NVME_LBART_ATTRIB_HIDE: u8 = 1 << 1;

#[repr(C, packed)]
pub struct NvmeIdPowerState {
    max_power: le<u16>, /* centiwatts */
    rsvd2: u16,
    entry_lat: le<u32>, /* microseconds */
    exit_lat: le<u32>,  /* microseconds */
    read_tput: u8,
    read_lat: u8,
    write_tput: u8,
    write_lat: u8,
    rsvd16: [u8; 16],
}

#[repr(C, packed)]
pub struct NvmeIdCtrl {
    pub vid: le<u16>,
    pub ssvid: le<u16>,
    pub sn: [u8; 20],
    pub mn: [u8; 40],
    pub fr: [u8; 8],
    pub rab: u8,
    pub ieee: [u8; 3],
    pub mic: u8,
    pub mdts: u8,
    rsvd78: [u8; 178],
    pub oacs: le<u16>,
    pub acl: u8,
    pub aerl: u8,
    pub frmw: u8,
    pub lpa: u8,
    pub elpe: u8,
    pub npss: u8,
    rsvd264: [u8; 248],
    pub sqes: u8,
    pub cqes: u8,
    pub rsvd514: [u8; 2],
    pub nn: le<u32>,
    pub oncs: le<u16>,
    pub fuses: le<u16>,
    pub fna: u8,
    pub vwc: u8,
    pub awun: le<u16>,
    pub awupf: le<u16>,
    rsvd530: [u8; 1518],
    pub psd: [NvmeIdPowerState; 32],
    pub vs: [u8; 1024],
}

#[repr(C, packed)]
pub struct NvmeLbaf {
    pub ms: le<u16>,
    pub ds: u8,
    pub rp: u8,
}

#[repr(C, packed)]
pub struct NvmeIdNs {
    pub nsze: le<u64>,
    pub ncap: le<u64>,
    pub nuse: le<u64>,
    pub nsfeat: u8,
    pub nlbaf: u8,
    pub flbas: u8,
    pub mc: u8,
    pub dpc: u8,
    pub dps: u8,
    rsvd30: [u8; 98],
    pub lbaf: [NvmeLbaf; 16],
    rsvd192: [u8; 192],
    pub vs: [u8; 3712],
}

#[derive(Default, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeRw {
    pub opcode: u8,
    pub flags: u8,
    pub command_id: u16,
    pub nsid: le<u32>,
    pub rsvd2: u64,
    pub metadata: le<u64>,
    pub prp1: le<u64>,
    pub prp2: le<u64>,
    pub slba: le<u64>,
    pub length: le<u16>,
    pub control: le<u16>,
    pub dsmgmt: le<u32>,
    pub reftag: le<u32>,
    pub apptag: le<u16>,
    pub appmask: le<u16>,
}

#[derive(Default, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeCommon {
    pub opcode: u8,
    pub flags: u8,
    pub command_id: u16,
    pub nsid: le<u32>,
    pub cdw2: [u32; 2],
    pub metadata: le<u64>,
    pub prp1: le<u64>,
    pub prp2: le<u64>,
    pub cdw10: [u32; 6],
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
pub union NvmeCommand {
    pub common: NvmeCommon,
    pub rw: NvmeRw,
    pub identify: NvmeIdentify,
    pub features: NvmeFeatures,
    pub create_cq: NvmeCreateCq,
    pub create_sq: NvmeCreateSq,
}

impl NvmeCommand {
    pub fn new_flush(nsid: u32) -> Self {
        Self {
            common: NvmeCommon {
                opcode: NvmeOpcode::flush as _,
                nsid: nsid.into(),
                ..NvmeCommon::default()
            },
        }
    }
}

impl Default for NvmeCommand {
    fn default() -> Self {
        Self {
            common: NvmeCommon::default(),
        }
    }
}



pub struct NvmeNamespace {
    pub id: u32,
    pub lba_shift: u32,
}


pub const NVME_CC_ENABLE: u32 = 1 << 0;
pub const NVME_CC_CSS_NVM: u32 = 0 << 4;
pub const NVME_CC_MPS_SHIFT: u32 = 7;
pub const NVME_CC_ARB_RR: u32 = 0 << 11;
pub const NVME_CC_ARB_WRRU: u32 = 1 << 11;
pub const NVME_CC_ARB_VS: u32 = 7 << 11;
pub const NVME_CC_SHN_NONE: u32 = 0 << 14;
pub const NVME_CC_SHN_NORMAL: u32 = 1 << 14;
pub const NVME_CC_SHN_ABRUPT: u32 = 2 << 14;
pub const NVME_CC_IOSQES: u32 = 6 << 16;
pub const NVME_CC_IOCQES: u32 = 4 << 20;
pub const NVME_CSTS_RDY: u32 = 1 << 0;
pub const NVME_CSTS_CFS: u32 = 1 << 1;
pub const NVME_CSTS_SHST_NORMAL: u32 = 0 << 2;
pub const NVME_CSTS_SHST_OCCUR: u32 = 1 << 2;
pub const NVME_CSTS_SHST_CMPLT: u32 = 2 << 2;

// TODO Prefix constants with something.
pub const OFFSET_CAP: usize = 0x00;
pub const OFFSET_CC: usize = 0x14;
pub const OFFSET_CSTS: usize = 0x1c;
pub const OFFSET_AQA: usize = 0x24;
pub const OFFSET_ASQ: usize = 0x28;
pub const OFFSET_ACQ: usize = 0x30;

pub const FEAT_LBA_RANGE: u32 = 0x03;
