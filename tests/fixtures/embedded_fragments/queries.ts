export const AccountWithSettings = gql/* GraphQL */ `
  fragment Auth_AccountWithSettings on Account {
    id
    permissions
    settings {
      ...Auth_FullAccountSettings
    }
  }

  fragment Auth_FullAccountSettings on AccountSettings {
    filterExplicit
    restrictBlockTracks
    restrictDiscoverMusic
    restrictEditMusic
    restrictUnpairingFromPairedDevices
  }
`;
