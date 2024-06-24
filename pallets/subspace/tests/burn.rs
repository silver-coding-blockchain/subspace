mod mock;
use frame_support::assert_ok;

use mock::*;
use pallet_subspace::{
    global::BurnConfiguration, AdjustmentAlpha, Burn, MaxRegistrationsPerBlock,
    TargetRegistrationsInterval, TargetRegistrationsPerInterval,
};
use sp_core::U256;

// test subnet specific burn
#[test]
fn test_local_subnet_burn() {
    new_test_ext().execute_with(|| {
        let min_burn = to_nano(10);
        let max_burn = to_nano(1000);

        let burn_config = BurnConfiguration {
            min_burn,
            max_burn,
            ..BurnConfiguration::<Test>::default()
        };

        assert_ok!(burn_config.apply());

        MaxRegistrationsPerBlock::<Test>::set(5);
        let target_reg_interval = 200;
        let target_reg_per_interval = 25;

        // register the general subnet
        assert_ok!(register_module(0, U256::from(0), to_nano(20)));
        AdjustmentAlpha::<Test>::insert(0, 200);
        TargetRegistrationsPerInterval::<Test>::insert(0, 25);
        // Adjust max registrations per block to a high number.
        // We will be doing "registration raid"
        TargetRegistrationsInterval::<Test>::insert(0, target_reg_interval); // for the netuid 0
        TargetRegistrationsPerInterval::<Test>::insert(0, target_reg_per_interval); // for the netuid 0

        // register 500 modules on yuma subnet
        let netuid = 1;
        let n = 300;
        let initial_stake: u64 = to_nano(500);

        MaxRegistrationsPerBlock::<Test>::set(1000);
        // this will perform 300 registrations and step in between
        for i in 1..n {
            // this registers five in block
            assert_ok!(register_module(netuid, U256::from(i), initial_stake));
            if i % 5 == 0 {
                // after that we step 30 blocks
                // meaning that the average registration per block is 0.166..
                TargetRegistrationsInterval::<Test>::insert(netuid, target_reg_interval); // for the netuid 0
                TargetRegistrationsPerInterval::<Test>::insert(netuid, target_reg_per_interval); // fo
                step_block(30);
            }
        }

        // We are at block 1,8 k now.
        // We performed 300 registrations
        // this means avg.  0.166.. per block
        // burn has incrased by 90% > up

        let subnet_zero_burn = Burn::<Test>::get(0);
        assert_eq!(subnet_zero_burn, min_burn);
        let subnet_one_burn = Burn::<Test>::get(1);
        assert!(min_burn < subnet_one_burn && subnet_one_burn < max_burn);
    });
}
