package app

import (
	"fmt"
	upgradetypes "github.com/cosmos/cosmos-sdk/x/upgrade/types"
	"union/app/upgrades"
	"union/app/upgrades/v0_3_0"
	"union/app/upgrades/v0_7_0"
)

var Upgrades = []upgrades.Upgrade{v0_3_0.Upgrade, v0_7_0.Upgrade}

// configure store loader that checks if version == upgradeHeight and applies store upgrades
func (app *UnionApp) setupUpgradeStoreLoaders() {
	upgradeInfo, err := app.UpgradeKeeper.ReadUpgradeInfoFromDisk()
	if err != nil {
		panic(fmt.Sprintf("failed to read upgrade info from disk %s", err))
	}

	if app.UpgradeKeeper.IsSkipHeight(upgradeInfo.Height) {
		return
	}

	for _, upgrade := range Upgrades {
		if upgradeInfo.Name == upgrade.UpgradeName {
			app.SetStoreLoader(upgradetypes.UpgradeStoreLoader(upgradeInfo.Height, &upgrade.StoreUpgrades))
		}
	}
}

func (app *UnionApp) setupUpgradeHandlers() {
	for _, upgrade := range Upgrades {
		app.UpgradeKeeper.SetUpgradeHandler(
			upgrade.UpgradeName,
			upgrade.CreateUpgradeHandler(
				app.mm,
				app.configurator,
			),
		)
	}
}
