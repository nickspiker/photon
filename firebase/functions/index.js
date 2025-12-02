const admin = require('firebase-admin');
admin.initializeApp();

// Simple FCM topic poke - FGTW calls this when any peer's IP changes
// All Android devices subscribed to 'peer_updates' topic receive a push
exports.poke = async (req, res) => {
  try {
    await admin.messaging().send({
      topic: 'peer_updates',
      data: { type: 'peer_update' },
      android: { priority: 'high' }
    });
    res.status(200).send('ok');
  } catch (err) {
    console.error('FCM send error:', err);
    res.status(500).send('error');
  }
};
