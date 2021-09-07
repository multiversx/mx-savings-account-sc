# sc-savings-account-rs

Savings Account design for Elrond Network, in which we use Liquid Staking positions from the Delegation SCs as borrow payment (through Semi-Fungible Tokens), and we lend stablecoins.  

Some modules are copied and slightly modified from the Lending/Borrowing SCs: https://github.com/ElrondNetwork/elrond-lend-rs

## External Components

The SC will need some external components to work propoerly:
- A price aggregator, which will be used the query the EGLD price in dollars at any point in time
- Exchange smart contract, which will be used to swap EGLD with stablecoins
- A delegation smart contract that uses Liquid Staking positions. 

Liquid staking is a concept in which the delegator receives some SFTs, representing the "locked" EGLD. Staking rewards claiming will be done through the SFTs instead. So a staking position can even be split among multiple accounts by simply transfering part of said SFTs.  

**In the current version, only the Price Aggregator integration is complete**

## Configurable parameters

The SC has the following configurable parameters:
- baseBorrowRate - The minimum borrow rate
- borrowRateUnderOptimalFactor - the factor that is used to adjust the borrow rate if it's under optimal utilisation
- borrowRateOverOptimalFactor - the factor that is used to adjust the borrow rate if it's higher than optimal utilisation
- optimalUtilisation - the optimal utilisation
- reserveFactor - Not needed?

The utilisation rate is defined as follows:

$utilisationRate = \frac{borrowedAmount}{totalDeposit}$

If the `utilisationRate` is lower than `optimalUtilisation`, then the borrow rate is defined by the following formula:

$borrowRate = baseBorrowRate + \frac{utilisationRate}{optimalUtilisation} * borrowRateUnderOptimalFactor$

If it's higher, then borrow rate is:

$borrowRate = baseBorrowRate + borrowRateUnderOptimalFactor + \frac{utilisationRate - optimalUtilisation}{1 - optimalUtilisation} * borrowRateOverOptimalFactor$

The deposit rate is defined as:

$depositRate = utilisationRate^2 * borrowRate * (1 - reserveFactor)$

## Actors

There are three main actors in this SC:
- lenders
- borrowers
- liquidators

### Lenders

Lenders are those that deposit their stablecoins. They can retrieve them later with an added interest rate, so this can be a passive source of income, like an investment.  

Where do these "extra" stablecoins come from? The SC does not mint them, but instead, they use the liquid staking positions collateralized by the borrowers to claim staking rewards. These rewards are in EGLD, so then we use an Exchange smart contract to swap these EGLD tokens to stablecoins.  

The amount of stablecoins received at withdrawal time is given by the following formula, which yields a `depositRate %` of the initial deposit amount per year:  

$withdrawalAmount = (1 + \frac{secondsSinceDeposit}{secondsInYear} * depositRate) * initialAmount$

At deposit time, the lenders receive 1:1 "Lend" SFTs for each token deposited, which are then used as payment for withdrawal. Lenders can also do partial withdrawals.  

### Borrowers

Borrowers are those that use their liquid staking positions as collateral to borrow stablecoins. The amount of borrowed tokens is defined by the following formula:

$borrowedAmount = borrowRate * collateralValue$

Where collateral value is the value in dollars of the deposited tokens, or rather, of the locked/staked EGLD they represent.  

To regain their liquid staking tokens, borrowers have to repay the initial borrowed amount, plus an extra amount known as "debt". The debt is calculated as follows:

$debtAmount = (\frac{secondsSinceBorrow}{secondsInYear} * borrowRate) * borrowAmount$

This yields a debt of `borrowRate %` per year.  

Borrows can do both a full repay or a partial repay.  

### Liquidators

NOT IMPLEMENTED YET

Liquidators are those that watch over the collateralized staking positions and liquidate them if they become too "risky". Each position is given a factor known as the "health factor". When a position's health factor becomes too low, it can be liquidated, which means anyone can buy the collateralized liquid staking tokens for a certain amount.  

