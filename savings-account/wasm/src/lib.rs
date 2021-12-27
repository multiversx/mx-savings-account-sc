////////////////////////////////////////////////////
////////////////// AUTO-GENERATED //////////////////
////////////////////////////////////////////////////

#![no_std]

elrond_wasm_node::wasm_endpoints! {
    savings_account
    (
        init
        callBack
        borrow
        calculateTotalLenderRewards
        claimStakingRewards
        convertStakingTokenToStablecoin
        getAggregatorAddress
        getBorowedAmount
        getBorrowTokenId
        getDelegationScAddress
        getDexSwapScAddress
        getLastCalculateRewardsEpoch
        getLastStakingRewardsClaimEpoch
        getLastStakingTokenConvertEpoch
        getLendTokenId
        getLendedAmount
        getLenderClaimableRewards
        getLenderRewardsPercentagePerEpoch
        getLiquidStakingTokenId
        getLoadToValuePercentage
        getStablecoinReserves
        getStablecoinTokenId
        getStakedTokenId
        getUnclaimedRewards
        issueBorrowToken
        issueLendToken
        lend
        lenderClaimRewards
        receiveStakingRewards
        receive_stablecoin_after_convert
        repay
        setBorrowTokenRoles
        setLendTokenRoles
        setPriceAggregatorAddress
        withdraw
    )
}
