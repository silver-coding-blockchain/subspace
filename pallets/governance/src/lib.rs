//! The Governance pallet.
#![cfg_attr(not(feature = "std"), no_std)]

pub mod dao;
pub mod migrations;
pub mod proposal;
pub mod voting;

use frame_support::{
    dispatch::DispatchResult,
    ensure,
    sp_runtime::{DispatchError, Percent},
};
use frame_system::pallet_prelude::OriginFor;
use sp_std::vec::Vec;

pub use pallet::*;
pub use pallet_governance_api::*;
pub use proposal::{Proposal, ProposalData, ProposalId, ProposalStatus, UnrewardedProposal};

type SubnetId = u16;

#[frame_support::pallet]
pub mod pallet {
    #![allow(clippy::too_many_arguments)]

    use crate::{dao::CuratorApplication, *};
    use frame_support::{
        pallet_prelude::{ValueQuery, *},
        traits::{Currency, StorageInstance},
        PalletId,
    };
    use frame_system::pallet_prelude::{ensure_signed, BlockNumberFor};
    use pallet_subspace::DefaultKey;
    use sp_runtime::traits::AccountIdConversion;

    const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    #[pallet::config(with_default)]
    pub trait Config: frame_system::Config + pallet_subspace::Config {
        /// This pallet's ID, used for generating the treasury account ID.
        #[pallet::constant]
        type PalletId: Get<PalletId>;

        /// The events emitted on proposal changes.
        #[pallet::no_default_bounds]
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// Currency type that will be used to place deposits on modules
        type Currency: Currency<Self::AccountId> + Send + Sync;
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(block_number: BlockNumberFor<T>) -> Weight {
            let block_number: u64 =
                block_number.try_into().ok().expect("blockchain won't pass 2 ^ 64 blocks");

            proposal::tick_proposals::<T>(block_number);
            proposal::tick_proposal_rewards::<T>(block_number);

            Weight::zero()
        }
    }

    impl<T: Config> StorageInstance for Pallet<T> {
        const STORAGE_PREFIX: &'static str = "Governance";

        fn pallet_prefix() -> &'static str {
            "Governance"
        }
    }

    // ---------------------------------
    // Proposals
    // ---------------------------------

    #[pallet::storage]
    pub type GlobalGovernanceConfig<T: Config> =
        StorageValue<_, GovernanceConfiguration, ValueQuery>;

    #[pallet::type_value]
    pub fn DefaultSubnetGovernanceConfig<T: Config>() -> GovernanceConfiguration {
        GovernanceConfiguration {
            vote_mode: VoteMode::Authority,
            ..Default::default()
        }
    }

    #[pallet::storage]
    pub type SubnetGovernanceConfig<T: Config> = StorageMap<
        _,
        Identity,
        SubnetId,
        GovernanceConfiguration,
        ValueQuery,
        DefaultSubnetGovernanceConfig<T>,
    >;

    /// A map of all proposals, indexed by their IDs.
    #[pallet::storage]
    pub type Proposals<T: Config> = StorageMap<_, Identity, ProposalId, Proposal<T>>;

    /// A map relating all modules and the stakers that are currently **NOT** delegating their
    /// voting power.
    ///
    /// Indexed by the **staked** module and the subnet the stake is allocated to, the value is a
    /// set of all modules that are delegating their voting power on that subnet.
    #[pallet::storage]
    pub type NotDelegatingVotingPower<T: Config> =
        StorageValue<_, BoundedBTreeSet<T::AccountId, ConstU32<{ u32::MAX }>>, ValueQuery>;

    #[pallet::storage]
    pub type UnrewardedProposals<T: Config> =
        StorageMap<_, Identity, ProposalId, UnrewardedProposal<T>>;

    #[pallet::type_value] // This has to be different than DefaultKey, so we are not conflicting in tests.
    pub fn DefaultDaoTreasuryAddress<T: Config>() -> T::AccountId {
        <T as Config>::PalletId::get().into_account_truncating()
    }

    #[pallet::storage]
    pub type DaoTreasuryAddress<T: Config> =
        StorageValue<_, T::AccountId, ValueQuery, DefaultDaoTreasuryAddress<T>>;

    #[pallet::type_value]
    pub fn DefaultDaoTreasuryDistribution<T: Config>() -> Percent {
        Percent::from_percent(5u8)
    }

    #[pallet::storage]
    pub type DaoTreasuryDistribution<T: Config> =
        StorageValue<_, Percent, ValueQuery, DefaultDaoTreasuryDistribution<T>>;

    // ---------------------------------
    // Dao
    // ---------------------------------

    #[pallet::type_value]
    pub fn DefaultGeneralSubnetApplicationCost<T: Config>() -> u64 {
        1_000_000_000_000 // 1_000 $COMAI
    }

    #[pallet::storage]
    pub type GeneralSubnetApplicationCost<T: Config> =
        StorageValue<_, u64, ValueQuery, DefaultGeneralSubnetApplicationCost<T>>;

    #[pallet::storage]
    pub type CuratorApplications<T: Config> = StorageMap<_, Identity, u64, CuratorApplication<T>>;

    // whitelist for the base subnet (netuid 0)
    #[pallet::storage]
    pub type LegitWhitelist<T: Config> = StorageMap<_, Identity, T::AccountId, u8, ValueQuery>;

    #[pallet::storage]
    pub type Curator<T: Config> = StorageValue<_, T::AccountId, ValueQuery, DefaultKey<T>>;

    // Add benchmarks for the pallet
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight((Weight::zero(), DispatchClass::Normal, Pays::No))]
        pub fn add_global_params_proposal(
            origin: OriginFor<T>,
            data: Vec<u8>,
            max_name_length: u16,
            min_name_length: u16,
            max_allowed_subnets: u16,
            max_allowed_modules: u16,
            max_registrations_per_block: u16,
            max_allowed_weights: u16,
            max_burn: u64,
            min_burn: u64,
            floor_delegation_fee: Percent,
            floor_founder_share: u8,
            min_weight_stake: u64,
            curator: T::AccountId,
            subnet_stake_threshold: Percent,
            proposal_cost: u64,
            proposal_expiration: u32,
            general_subnet_application_cost: u64,
        ) -> DispatchResult {
            let mut params = pallet_subspace::Pallet::<T>::global_params();
            params.max_name_length = max_name_length;
            params.min_name_length = min_name_length;
            params.max_allowed_subnets = max_allowed_subnets;
            params.max_allowed_modules = max_allowed_modules;
            params.max_registrations_per_block = max_registrations_per_block;
            params.max_allowed_weights = max_allowed_weights;
            params.floor_delegation_fee = floor_delegation_fee;
            params.floor_founder_share = floor_founder_share;
            params.min_weight_stake = min_weight_stake;
            params.curator = curator;
            params.subnet_stake_threshold = subnet_stake_threshold;
            params.governance_config.proposal_cost = proposal_cost;
            params.governance_config.proposal_expiration = proposal_expiration;
            params.general_subnet_application_cost = general_subnet_application_cost;

            params.burn_config.min_burn = min_burn;
            params.burn_config.max_burn = max_burn;

            Self::do_add_global_params_proposal(origin, data, params)
        }

        #[pallet::call_index(1)]
        #[pallet::weight((Weight::zero(), DispatchClass::Normal, Pays::No))]
        pub fn add_subnet_params_proposal(
            origin: OriginFor<T>,
            subnet_id: u16,
            data: Vec<u8>,
            founder: T::AccountId,
            name: BoundedVec<u8, ConstU32<256>>,
            founder_share: u16,
            immunity_period: u16,
            incentive_ratio: u16,
            max_allowed_uids: u16,
            max_allowed_weights: u16,
            min_allowed_weights: u16,
            min_stake: u64,
            max_weight_age: u64,
            tempo: u16,
            trust_ratio: u16,
            maximum_set_weight_calls_per_epoch: u16,
            vote_mode: VoteMode,
            bonds_ma: u64,
            target_registrations_interval: u16,
            target_registrations_per_interval: u16,
            max_registrations_per_interval: u16,
            adjustment_alpha: u64,
        ) -> DispatchResult {
            let mut params = pallet_subspace::Pallet::subnet_params(subnet_id);
            params.founder = founder;
            params.name = name;
            params.founder_share = founder_share;
            params.immunity_period = immunity_period;
            params.incentive_ratio = incentive_ratio;
            params.max_allowed_uids = max_allowed_uids;
            params.max_allowed_weights = max_allowed_weights;
            params.min_allowed_weights = min_allowed_weights;
            params.min_stake = min_stake;
            params.max_weight_age = max_weight_age;
            params.tempo = tempo;
            params.trust_ratio = trust_ratio;
            params.maximum_set_weight_calls_per_epoch = maximum_set_weight_calls_per_epoch;
            params.governance_config.vote_mode = vote_mode;
            params.bonds_ma = bonds_ma;
            params.target_registrations_interval = target_registrations_interval;
            params.target_registrations_per_interval = target_registrations_per_interval;
            params.max_registrations_per_interval = max_registrations_per_interval;
            params.adjustment_alpha = adjustment_alpha;

            Self::do_add_subnet_params_proposal(origin, subnet_id, data, params)
        }

        #[pallet::call_index(2)]
        #[pallet::weight((Weight::zero(), DispatchClass::Normal, Pays::No))]
        pub fn add_global_custom_proposal(origin: OriginFor<T>, data: Vec<u8>) -> DispatchResult {
            Self::do_add_global_custom_proposal(origin, data)
        }

        #[pallet::call_index(3)]
        #[pallet::weight((Weight::zero(), DispatchClass::Normal, Pays::No))]
        pub fn add_subnet_custom_proposal(
            origin: OriginFor<T>,
            subnet_id: u16,
            data: Vec<u8>,
        ) -> DispatchResult {
            Self::do_add_subnet_custom_proposal(origin, subnet_id, data)
        }

        #[pallet::call_index(4)]
        #[pallet::weight((Weight::zero(), DispatchClass::Normal, Pays::No))]
        pub fn add_transfer_dao_treasury_proposal(
            origin: OriginFor<T>,
            data: Vec<u8>,
            value: u64,
            dest: T::AccountId,
        ) -> DispatchResult {
            Self::do_add_transfer_dao_treasury_proposal(origin, data, value, dest)
        }

        // Once benchmarked, provide more accurate weight info.
        // This has to pay fee, so very low stake keys don't spam the voting system.
        #[pallet::call_index(5)]
        #[pallet::weight(Weight::from_parts(22_732_000, 6825)
        .saturating_add(T::DbWeight::get().reads(4_u64))
        .saturating_add(T::DbWeight::get().writes(1_u64))
        )]
        pub fn vote_proposal(
            origin: OriginFor<T>,
            proposal_id: u64,
            agree: bool,
        ) -> DispatchResult {
            Self::do_vote_proposal(origin, proposal_id, agree)
        }

        #[pallet::call_index(6)]
        #[pallet::weight((Weight::zero(), DispatchClass::Normal, Pays::No))]
        pub fn remove_vote_proposal(origin: OriginFor<T>, proposal_id: u64) -> DispatchResult {
            Self::do_remove_vote_proposal(origin, proposal_id)
        }

        #[pallet::call_index(7)]
        #[pallet::weight((Weight::zero(), DispatchClass::Normal, Pays::No))]
        pub fn enable_vote_power_delegation(origin: OriginFor<T>) -> DispatchResult {
            let key = ensure_signed(origin)?;
            Self::update_delegating_voting_power(&key, true)
        }

        #[pallet::call_index(8)]
        #[pallet::weight((Weight::zero(), DispatchClass::Normal, Pays::No))]
        pub fn disable_vote_power_delegation(origin: OriginFor<T>) -> DispatchResult {
            let key = ensure_signed(origin)?;
            Self::update_delegating_voting_power(&key, false)
        }

        // ---------------------------------
        // Subnet 0 DAO
        // ---------------------------------

        // TODO:
        // add the benchmarks later
        #[pallet::call_index(9)]
        #[pallet::weight((Weight::zero(), DispatchClass::Normal, Pays::No))]
        pub fn add_dao_application(
            origin: OriginFor<T>,
            application_key: T::AccountId,
            data: Vec<u8>,
        ) -> DispatchResult {
            Self::do_add_dao_application(origin, application_key, data)
        }

        #[pallet::call_index(10)]
        #[pallet::weight((Weight::zero(), DispatchClass::Normal, Pays::No))]
        pub fn refuse_dao_application(origin: OriginFor<T>, id: u64) -> DispatchResult {
            Self::do_refuse_dao_application(origin, id)
        }

        #[pallet::call_index(11)]
        #[pallet::weight((Weight::zero(), DispatchClass::Normal, Pays::No))]
        pub fn add_to_whitelist(
            origin: OriginFor<T>,
            module_key: T::AccountId,
            recommended_weight: u8,
        ) -> DispatchResult {
            Self::do_add_to_whitelist(origin, module_key, recommended_weight)
        }

        #[pallet::call_index(12)]
        #[pallet::weight((Weight::zero(), DispatchClass::Normal, Pays::No))]
        pub fn remove_from_whitelist(
            origin: OriginFor<T>,
            module_key: T::AccountId,
        ) -> DispatchResult {
            Self::do_remove_from_whitelist(origin, module_key)
        }
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(crate) fn deposit_event)]
    pub enum Event<T: Config> {
        ProposalCreated(ProposalId),

        ProposalAccepted(ProposalId),
        ProposalRefused(ProposalId),
        ProposalExpired(ProposalId),

        ProposalVoted(u64, T::AccountId, bool),
        ProposalVoteUnregistered(u64, T::AccountId),

        WhitelistModuleAdded(T::AccountId), /* --- Event created when a module account has been
                                             * added to the whitelist. */
        WhitelistModuleRemoved(T::AccountId), /* --- Event created when a module account has
                                               * been removed from the whitelist. */
        ApplicationCreated(u64),
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The proposal is already finished. Do not retry.
        ProposalIsFinished,
        /// Invalid parameters were provided to the finalization process.
        InvalidProposalFinalizationParameters,
        /// Invalid parameters were provided to the voting process.
        InvalidProposalVotingParameters,
        /// Negative proposal cost when setting global or subnet governance configuration.
        InvalidProposalCost,
        /// Negative expiration when setting global or subnet governance configuration.
        InvalidProposalExpiration,
        /// Key doesn't have enough tokens to create a proposal.
        NotEnoughBalanceToPropose,
        /// Proposal data is empty.
        ProposalDataTooSmall,
        /// Proposal data is bigger than 256 characters.
        ProposalDataTooLarge,
        /// The staked module is already delegating for 2 ^ 32 keys.
        ModuleDelegatingForMaxStakers,
        /// Proposal with given id doesn't exist.
        ProposalNotFound,
        /// Proposal was either accepted, refused or expired and cannot accept votes.
        ProposalClosed,
        /// Proposal data isn't composed by valid UTF-8 characters.
        InvalidProposalData,
        /// Invalid value given when transforming a u64 into T::Currency.
        InvalidCurrencyConversionValue,
        /// Dao Treasury doesn't have enough funds to be transferred.
        InsufficientDaoTreasuryFunds,
        /// Subnet is on Authority Mode.
        NotVoteMode,
        /// Key has already voted on given Proposal.
        AlreadyVoted,
        /// Key hasn't voted on given Proposal.
        NotVoted,
        /// Key doesn't have enough stake to vote.
        InsufficientStake,
        /// The voter is delegating its voting power to their staked modules. Disable voting power
        /// delegation.
        VoterIsDelegatingVotingPower,
        /// The network vote mode must be authority for changes to be imposed.
        VoteModeIsNotAuthority,
        /// An internal error occurred, probably relating to the size of the bounded sets.
        InternalError,

        // DAO / Governance
        ApplicationTooSmall,
        ApplicationTooLarge,
        ApplicationNotPending,
        InvalidApplication,
        NotEnoughtBalnceToApply,
        InvalidRecommendedWeight,
        NotCurator, /* --- Thrown when the user tries to set the curator and is not the
                     * curator */
        ApplicationNotFound,
        AlreadyWhitelisted, /* --- Thrown when the user tries to whitelist an account that is
                             * already whitelisted. */
        NotWhitelisted, /* --- Thrown when the user tries to remove an account from the
                         * whitelist that is not whitelisted. */
        CouldNotConvertToBalance,
    }
}

impl<T: Config> Pallet<T> {
    pub fn validate(
        config: GovernanceConfiguration,
    ) -> Result<GovernanceConfiguration, DispatchError> {
        ensure!(config.proposal_cost > 0, Error::<T>::InvalidProposalCost);
        ensure!(
            config.proposal_expiration > 0,
            Error::<T>::InvalidProposalExpiration
        );
        Ok(config)
    }
}

impl<T: Config> Pallet<T> {
    pub fn is_delegating_voting_power(delegator: &T::AccountId) -> bool {
        !NotDelegatingVotingPower::<T>::get().contains(delegator)
    }

    pub fn update_delegating_voting_power(
        delegator: &T::AccountId,
        delegating: bool,
    ) -> DispatchResult {
        NotDelegatingVotingPower::<T>::mutate(|delegators| {
            if !delegating {
                delegators
                    .try_insert(delegator.clone())
                    .map(|_| ())
                    .map_err(|_| Error::<T>::InternalError.into())
            } else {
                delegators.remove(delegator);
                Ok(())
            }
        })
    }

    pub fn update_global_governance_configuration(
        config: GovernanceConfiguration,
    ) -> DispatchResult {
        let config = Self::validate(config)?;
        GlobalGovernanceConfig::<T>::set(config);
        Ok(())
    }

    pub fn update_subnet_governance_configuration(
        subnet_id: u16,
        config: GovernanceConfiguration,
    ) -> DispatchResult {
        let config = Self::validate(config)?;
        SubnetGovernanceConfig::<T>::set(subnet_id, config);
        Ok(())
    }

    pub fn handle_subnet_removal(subnet_id: u16) {
        SubnetGovernanceConfig::<T>::remove(subnet_id);
    }
}
